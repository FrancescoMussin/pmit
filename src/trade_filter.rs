use crate::polymarket::Trade;

pub struct TradeFilter;

impl TradeFilter {
    pub fn new() -> Self {
        Self
    }

    pub fn should_print_trade(&self, trade: &Trade) -> bool {
        let title = trade.title.as_deref().unwrap_or("").to_ascii_lowercase();
        !title.contains("up or down") && !title.contains("temperature") && !title.contains("vs")
    }
}
