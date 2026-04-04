use serde::{Deserialize, Serialize};
use super::market::Market;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub title: String,
    pub active: bool,
    #[serde(default)]
    pub markets: Vec<Market>,
}
