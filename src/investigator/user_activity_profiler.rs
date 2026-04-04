use crate::config::Config;
use crate::data_structures::WalletAddress;
use crate::database_handler::UserHistoryDatabaseHandler;
use crate::polymarket;
use crate::polymarket::Trade;
use crate::investigator::MarketDistributions;
use lru::LruCache;
use std::collections::HashMap;

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
        trades: Vec<Trade>,
        token_map: &HashMap<String, String>,
        market_distributions: &mut MarketDistributions,
        recent_users: &mut LruCache<WalletAddress, ()>,
        client: &reqwest::Client,
        user_history_db: &UserHistoryDatabaseHandler,
        config: &Config,
    ) {
        // iterate over all trades
        for trade in trades {
            // check if the trade's token ID corresponds to any monitored condition
            if let Some(condition_id) = token_map.get(trade.asset.as_str()) {
                let p_value = market_distributions.score_trade(condition_id, &trade).unwrap_or(1.0);
                
                // we profile the trade.
                self.profile_trade(
                    recent_users,
                    client,
                    user_history_db,
                    config,
                    trade,
                    p_value,
                );
            }
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
        trade: Trade,
        p_value: f64,
    ) {
        let total_value = trade.size * trade.price;
        let bet_title = trade.title.as_deref().unwrap_or("Unknown Market");
        let bet_outcome = trade.outcome.as_deref().unwrap_or("N/A");
        let total_profit = total_value * (1.0 / trade.price - 1.0);
        tracing::info!(
            "\n\
            ======================================= 🚨 TRADE 🚨 =======================================\n\
            Market:   {}\n\
            Outcome:  {}\n\
            Value:    ${:.2} ({:.2} shares @ ${:.2})\n\
            Profit:   ${:.2}\n\
            ===========================================================================================",
            bet_title,
            bet_outcome,
            total_value,
            trade.size,
            trade.price,
            total_profit
        );

        // Only profile users if the probability of the trade is low enough (anomaly).
        if p_value < config.anomaly_probability_threshold {
            // If we haven't checked this user recently, fetch their history in background.
            if !recent_users.contains(&trade.maker_address) {
                tracing::info!(
                    "\n\
                    >> 📊 PROFILER ALERT!\n\
                    >> Anomalously large trade (p={:.4}) by: {}\n\
                    >> Fetching activity profile...",
                    p_value,
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

                // we fetch the user's activity on polymarket in the background
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
                                .insert_user_activity_snapshot(
                                    address_clone.to_string(),
                                    activity.clone(),
                                )
                                .await
                            {
                                tracing::error!(
                                    "\n\
                                    >> ❌ PERSIST ERROR\n\
                                    >> Address: {}\n\
                                    >> Error:   {:?}",
                                    address_clone,
                                    e
                                );
                            }

                            // we generate a preview of the fetched activity for logging purposes.
                            // We serialize the activity to JSON and take the first 200 characters
                            // to avoid overwhelming the logs
                            let json_str = serde_json::to_string(&activity).unwrap_or_default();
                            let preview: String = json_str.chars().take(200).collect();
                            tracing::info!(
                                "\n\
                                >> ✅ PROFILE FETCHED\n\
                                >> Address: {}\n\
                                >> Preview: {}...",
                                address_clone,
                                preview
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "\n\
                                >> ❌ FETCH ERROR\n\
                                >> Address: {}\n\
                                >> Error:   {:?}",
                                address_clone,
                                e
                            );
                        }
                    }
                });
            }
        }
    }
}
