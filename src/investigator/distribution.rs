use crate::polymarket::Trade;
use std::collections::HashMap;

/// Master struct holding the distributions of all monitored markets.
#[derive(Default)]
pub struct MarketDistributions {
    pub distributions: HashMap<String, SingleMarketDistribution>,
}

impl MarketDistributions {
    pub fn new() -> Self {
        Self {
            distributions: HashMap::new(),
        }
    }

    pub fn setup_market(&mut self, condition_id: String, alpha: f64) {
        self.distributions.insert(condition_id.clone(), SingleMarketDistribution::new(condition_id, alpha));
    }

    pub fn insert_historical_trade(&mut self, condition_id: &str, trade: &Trade) {
        if let Some(dist) = self.distributions.get_mut(condition_id) {
            dist.insert_historical_trade(trade);
        }
    }

    pub fn score_trade(&mut self, condition_id: &str, trade: &Trade) -> Option<f64> {
        self.distributions.get_mut(condition_id).map(|dist| dist.score_trade(trade))
    }
}

/// A struct that holds distributional data for a single market.
/// This tracks the Exponentially Weighted Moving Average (EWMA) and Variance of trade values (USD).
pub struct SingleMarketDistribution {
    pub condition_id: String,
    pub alpha: f64, // Decay factor
    pub ewma_val_mean: Option<f64>,
    pub ewma_val_variance: f64,
}

impl SingleMarketDistribution {
    pub fn new(condition_id: String, alpha: f64) -> Self {
        Self {
            condition_id,
            alpha,
            ewma_val_mean: None,
            ewma_val_variance: 0.0,
        }
    }

    /// Internal helper to update Welford's online algorithm with an exponential weight
    fn update_distribution(&mut self, val: f64) {
        if let Some(mean) = self.ewma_val_mean {
            // update the mean (numerically stable)
            let diff = val - mean;
            let new_mean = mean + self.alpha * diff;
            // update the variance (numerically stable)
            self.ewma_val_variance = (1.0 - self.alpha) * self.ewma_val_variance + self.alpha * diff * (val - new_mean);
            self.ewma_val_mean = Some(new_mean);
        } else {
            // initialize the mean and variance
            self.ewma_val_mean = Some(val);
            self.ewma_val_variance = 0.0;
        }
    }

    /// Add a historical trade to the distribution to build the initial profile.
    pub fn insert_historical_trade(&mut self, trade: &Trade) {
        // calculate the total value of the trade
        let total_value = trade.size * trade.price;
        // update the distribution
        self.update_distribution(total_value);
    }

    /// Given a new real-time trade, return a p-value bound indicating its rarity within the distribution.
    /// This uses Chebyshev's Inequality: P(|X - μ| >= kσ) <= 1/k^2.
    /// Returns 0.0 to 1.0 (the upper bound on the probability of such an extreme value).
    pub fn score_trade(&mut self, trade: &Trade) -> f64 {
        let total_value = trade.size * trade.price;
        let p_value = if let Some(mean) = self.ewma_val_mean {
            let diff_sq = (total_value - mean).powi(2);
            
            // Chebyshev's inequality provides a bound: P(|X - μ| >= kσ) <= 1/k^2
            // Since k = |total_value - mean| / σ, then k^2 = (total_value - mean)^2 / σ^2
            // So 1/k^2 = variance / diff_sq.
            // This inequality is only useful for k > 1, which means diff_sq > variance.
            if diff_sq > self.ewma_val_variance && diff_sq > 0.0 {
                (self.ewma_val_variance / diff_sq).min(1.0)
            } else {
                1.0 // Inequality is not useful for k <= 1
            }
        } else {
            1.0 // no history, so it cannot be considered an anomaly
        };

        // Incorporate the trade into the tracking curve AFTER it has been scored!
        self.update_distribution(total_value);

        p_value
    }
}
