use crate::config::Config;
use crate::data_structures::WalletAddress;
use crate::database_handler::UserHistoryDatabaseHandler;
use crate::exposure::ScoredTrade;
use crate::polymarket;
use crate::polymarket::Trade;
use lru::LruCache;

/// Struct for profiling user activity related to routed trades. This profiler focuses on fetching
/// and storing snapshots of user activity for makers of high-value routed trades.
pub struct UserActivityProfiler;

impl UserActivityProfiler {
    pub fn new() -> Self {
        Self
    }

    /// Profile all routed trades and fetch maker activity snapshots when needed.
    pub fn profile_batch(
        &self,
        relevant_trades: Vec<ScoredTrade>,
        recent_users: &mut LruCache<WalletAddress, ()>,
        client: &reqwest::Client,
        user_history_db: &UserHistoryDatabaseHandler,
        config: &Config,
    ) {
        // iterate over all relevant trades
        for scored_trade in relevant_trades {
            // get the exposure score
            let exposure_score = scored_trade.exposure_score;
            // get trade details
            let trade = scored_trade.trade;
            // we profile the trade.
            self.profile_trade(
                recent_users,
                client,
                user_history_db,
                config,
                exposure_score,
                trade,
            );
        }
    }

    /// Profile a single trade by checking if it meets the criteria for profiling and fetching user
    /// activity if needed.
    fn profile_trade(
        &self,
        recent_users: &mut LruCache<WalletAddress, ()>,
        client: &reqwest::Client,
        user_history_db: &UserHistoryDatabaseHandler,
        config: &Config,
        exposure_score: f64,
        trade: Trade,
    ) {
        let total_value = trade.size * trade.price;
        let bet_title = trade.title.as_deref().unwrap_or("Unknown Market");
        let bet_outcome = trade.outcome.as_deref().unwrap_or("N/A");

        println!(
            "🚨 TRADE: {} [{}] | Exposure: {:.3} | Share Size: {:.2} @ Price: ${:.2} (Value: ${:.2})",
            bet_title, bet_outcome, exposure_score, trade.size, trade.price, total_value
        );

        // Only profile users if their trade value is >= our threshold.
        if total_value >= config.large_trade_threshold {
            // If we haven't checked this user recently, fetch their history in background.
            if !recent_users.contains(&trade.maker_address) {
                println!(
                    "  📊 PROFILER ALERT! High-value routed trade by {}. Fetching activity profile...",
                    trade.maker_address
                );

                // Mark as seen before spawning, to avoid duplicate fetches in short bursts.
                recent_users.put(trade.maker_address.clone(), ());

                // We clone the necessary variables to move into the async task. This includes the
                // HTTP client, the maker's address, the user history database handler, and the API
                // URL from the config.
                let client_clone = client.clone();
                let address_clone = trade.maker_address.clone();
                let user_history_db_clone = user_history_db.clone();
                let api_url = config.polymarket_data_api_url.clone();

                tokio::spawn(async move {
                    match polymarket::fetch_user_activity(
                        &client_clone,
                        &api_url,
                        address_clone.as_str(),
                    )
                    .await
                    {
                        // Error handling as always
                        Ok(activity) => {
                            // we immediately persist the fetch activity snapshot to the database
                            // for future reference and potential training data, even if the
                            // subsequent processing or preview generation fails.
                            if let Err(e) = user_history_db_clone
                                .insert_user_activity_snapshot(address_clone.as_str(), &activity)
                            {
                                eprintln!(
                                    "  -> Failed to persist activity snapshot for {}: {:?}",
                                    address_clone, e
                                );
                            }

                            // we generate a preview of the fetched activity for logging purposes.
                            // We serialize the activity to JSON and take the first 200 characters
                            // to avoid overwhelming the logs
                            let json_str = serde_json::to_string(&activity).unwrap_or_default();
                            let preview: String = json_str.chars().take(200).collect();
                            println!(
                                "  -> Profile fetched for {}! Preview: {}...",
                                address_clone, preview
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "  -> Failed to fetch activity for {}: {:?}",
                                address_clone, e
                            );
                        }
                    }
                });
            }
        }
    }
}
