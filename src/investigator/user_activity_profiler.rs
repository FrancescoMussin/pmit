use crate::config::Config;
use crate::data_structures::WalletAddress;
use crate::polymarket::Trade;
use crate::investigator::MarketDistributions;
use crate::investigator::InvestigationRequest;
use lru::LruCache;
use std::collections::HashSet;
use tokio::sync::mpsc::UnboundedSender;

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
        monitored_tokens: &HashSet<String>,
        market_distributions: &mut MarketDistributions,
        recent_users: &mut LruCache<WalletAddress, ()>,
        tx: &UnboundedSender<InvestigationRequest>,
        config: &Config,
    ) {
        // iterate over all trades
        for trade in trades {
            // check if the trade's token ID corresponds to any monitored token
            if monitored_tokens.contains(trade.asset.as_str()) {
                let p_value = market_distributions.score_trade(trade.asset.as_str(), &trade).unwrap_or(1.0);
                
                // we profile the trade.
                self.profile_trade(
                    recent_users,
                    tx,
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
        tx: &UnboundedSender<InvestigationRequest>,
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
                    >> Queuing investigation request...",
                    p_value,
                    trade.maker_address
                );

                // Mark as seen before queuing, to avoid duplicate requests in short bursts.
                recent_users.put(trade.maker_address.clone(), ());

                let request = InvestigationRequest::new(trade, p_value);

                // send the request down the channel
                if let Err(e) = tx.send(request) {
                    tracing::error!("Failed to queue investigation request: {:?}", e);
                }
            }
        }
    }
}
