use crate::polymarket::Trade;

/// Struct to that helps in filtering out trades based on their titles, to exclude certain categories of trades from being printed.
pub struct TradeFilter;

impl TradeFilter {
    /// Creates a new instance of the `TradeFilter`.
    pub fn new() -> Self {
        Self
    }

    /// Returns `true` if the trade should be printed based on its title, and `false` if it should be filtered out.
    pub fn should_print_trade(&self, trade: &Trade) -> bool {
        let title = trade.title.as_deref().unwrap_or("").to_ascii_lowercase();
        !title.contains("up or down")
            && !title.contains("temperature")
            && !title.contains("vs")
            && !title.contains("Fifa")
            && !title.contains("nba")
            && !title.contains("f1")
    }
}
