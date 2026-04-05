use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents the side of a trade (buy or sell).
///
/// Serializes to lowercase for JSON compatibility with the Polymarket API
/// and for consistent database storage.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[serde(alias = "BUY")]
    Buy,
    #[serde(alias = "SELL")]
    Sell,
}

/// A blockchain wallet address.
///
/// Newtype wrapper for semantic clarity and type safety.
/// Serializes/deserializes transparently as a string.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct WalletAddress(String);

impl WalletAddress {
    pub fn new(addr: String) -> Self {
        Self(addr)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WalletAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A smart contract/token ID on the blockchain.
///
/// Newtype wrapper for semantic clarity and type safety.
/// Serializes/deserializes transparently as a string.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ContractId(String);

impl ContractId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContractId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A market condition ID on Polymarket.
///
/// Newtype wrapper for semantic clarity and type safety.
/// Serializes/deserializes transparently as a string.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ConditionId(String);

impl ConditionId {
    pub fn new(id: String) -> Self {
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConditionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Buy => write!(f, "buy"),
            Side::Sell => write!(f, "sell"),
        }
    }
}

/// Trade payload shared across ingestion, persistence, and downstream stages.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Trade {
    // The API key is `proxyWallet`; keep serde alias for compatibility.
    #[serde(alias = "proxyWallet")]
    pub maker_address: WalletAddress,
    pub side: Side,
    pub asset: ContractId,
    pub title: Option<String>,
    pub outcome: Option<String>,
    pub size: f64,
    pub price: f64,
    pub timestamp: u64,
    #[serde(alias = "transactionHash")]
    pub transaction_hash: String,
}

/// A trade from a user's past betting history.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PastTrade {
    #[serde(rename = "conditionId")]
    pub condition_id: ConditionId,
    pub size: f64,
    pub price: f64,
    pub side: Side,
    pub title: String,
    pub asset: ContractId,
}

/// A user's resolved/closed position on Polymarket.
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClosedPosition {
    pub condition_id: ConditionId,
    pub cur_price: f64,
    pub end_date: String,
    pub title: String,
    pub outcome: String,
    pub outcome_index: usize,
    pub realized_pnl: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ WalletAddress Tests ============

    #[test]
    fn test_wallet_address_deserialization() {
        let json = r#""0x1234567890abcdef""#;
        let wallet: WalletAddress = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(wallet.as_str(), "0x1234567890abcdef");
    }

    #[test]
    fn test_wallet_address_serialization() {
        let wallet = WalletAddress::new("0xaabbccdd".to_string());
        let json = serde_json::to_string(&wallet).expect("Failed to serialize");
        assert_eq!(json, r#""0xaabbccdd""#);
    }

    #[test]
    fn test_wallet_address_round_trip() {
        let original = WalletAddress::new("0x9876543210fedcba".to_string());
        let json = serde_json::to_string(&original).expect("Failed to serialize");
        let deserialized: WalletAddress =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_wallet_address_display() {
        let wallet = WalletAddress::new("0x123abc".to_string());
        assert_eq!(wallet.to_string(), "0x123abc");
    }

    // ============ ContractId Tests ============

    #[test]
    fn test_contract_id_deserialization() {
        let json = r#""USDC""#;
        let contract: ContractId = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(contract.as_str(), "USDC");
    }

    #[test]
    fn test_contract_id_serialization() {
        let contract = ContractId::new("ETH".to_string());
        let json = serde_json::to_string(&contract).expect("Failed to serialize");
        assert_eq!(json, r#""ETH""#);
    }

    #[test]
    fn test_contract_id_round_trip() {
        let original = ContractId::new("0xdac17f958d2ee523a2206206994597c13d831ec7".to_string());
        let json = serde_json::to_string(&original).expect("Failed to serialize");
        let deserialized: ContractId = serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_contract_id_display() {
        let contract = ContractId::new("USDT".to_string());
        assert_eq!(contract.to_string(), "USDT");
    }

    // ============ Side Tests ============

    #[test]
    fn test_side_buy_serialization() {
        let side = Side::Buy;
        let json = serde_json::to_string(&side).expect("Failed to serialize");
        assert_eq!(json, r#""buy""#);
    }

    #[test]
    fn test_side_sell_serialization() {
        let side = Side::Sell;
        let json = serde_json::to_string(&side).expect("Failed to serialize");
        assert_eq!(json, r#""sell""#);
    }

    #[test]
    fn test_side_buy_deserialization() {
        let json = r#""buy""#;
        let side: Side = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(side, Side::Buy);
    }

    #[test]
    fn test_side_sell_deserialization() {
        let json = r#""sell""#;
        let side: Side = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(side, Side::Sell);
    }

    #[test]
    fn test_side_buy_deserialization_uppercase() {
        let json = r#""BUY""#;
        let side: Side = serde_json::from_str(json).expect("Failed to deserialize uppercase");
        assert_eq!(side, Side::Buy);
    }

    #[test]
    fn test_side_sell_deserialization_uppercase() {
        let json = r#""SELL""#;
        let side: Side = serde_json::from_str(json).expect("Failed to deserialize uppercase");
        assert_eq!(side, Side::Sell);
    }

    #[test]
    fn test_side_display() {
        assert_eq!(Side::Buy.to_string(), "buy");
        assert_eq!(Side::Sell.to_string(), "sell");
    }

    // ============ Trade Tests ============

    #[test]
    fn test_trade_full_deserialization() {
        let json = r#"{
            "proxyWallet": "0x1234",
            "side": "buy",
            "asset": "USDC",
            "title": "Will Bitcoin reach $100k?",
            "outcome": "Yes",
            "size": 100.0,
            "price": 0.75,
            "timestamp": 1234567890,
            "transactionHash": "0xabcd"
        }"#;

        let trade: Trade = serde_json::from_str(json).expect("Failed to deserialize trade");
        assert_eq!(trade.maker_address.as_str(), "0x1234");
        assert_eq!(trade.side, Side::Buy);
        assert_eq!(trade.asset.as_str(), "USDC");
        assert_eq!(trade.title, Some("Will Bitcoin reach $100k?".to_string()));
        assert_eq!(trade.outcome, Some("Yes".to_string()));
        assert_eq!(trade.size, 100.0);
        assert_eq!(trade.price, 0.75);
        assert_eq!(trade.timestamp, 1234567890);
        assert_eq!(trade.transaction_hash, "0xabcd");
    }

    #[test]
    fn test_trade_with_optional_fields_none() {
        let json = r#"{
            "proxyWallet": "0x5678",
            "side": "sell",
            "asset": "ETH",
            "size": 50.0,
            "price": 0.25,
            "timestamp": 9876543210,
            "transactionHash": "0xef01"
        }"#;

        let trade: Trade = serde_json::from_str(json).expect("Failed to deserialize");
        assert_eq!(trade.maker_address.as_str(), "0x5678");
        assert_eq!(trade.side, Side::Sell);
        assert_eq!(trade.asset.as_str(), "ETH");
        assert_eq!(trade.title, None);
        assert_eq!(trade.outcome, None);
    }

    #[test]
    fn test_trade_round_trip_serialization() {
        let original_json = r#"{
            "proxyWallet": "0xaaaa",
            "side": "buy",
            "asset": "USDT",
            "title": "NBA Finals Winner",
            "outcome": "Lakers",
            "size": 250.5,
            "price": 0.55,
            "timestamp": 1111111111,
            "transactionHash": "0xffff"
        }"#;

        let trade: Trade = serde_json::from_str(original_json).expect("Failed to deserialize");
        let reserialized = serde_json::to_string(&trade).expect("Failed to serialize");
        let trade_again: Trade =
            serde_json::from_str(&reserialized).expect("Failed to deserialize again");

        assert_eq!(trade.maker_address, trade_again.maker_address);
        assert_eq!(trade.side, trade_again.side);
        assert_eq!(trade.asset, trade_again.asset);
        assert_eq!(trade.title, trade_again.title);
        assert_eq!(trade.outcome, trade_again.outcome);
        assert_eq!(trade.size, trade_again.size);
        assert_eq!(trade.price, trade_again.price);
        assert_eq!(trade.timestamp, trade_again.timestamp);
        assert_eq!(trade.transaction_hash, trade_again.transaction_hash);
    }

    #[test]
    fn test_trade_maker_address_alias() {
        // Test that "maker_address" key also works (in addition to "proxyWallet" alias)
        let json_with_alias = r#"{"proxyWallet": "0x9999", "side": "buy", "asset": "USDC", "size": 10.0, "price": 0.5, "timestamp": 100, "transactionHash": "0xaaaa"}"#;
        let json_with_direct = r#"{"maker_address": "0x9999", "side": "buy", "asset": "USDC", "size": 10.0, "price": 0.5, "timestamp": 100, "transactionHash": "0xaaaa"}"#;

        let trade_alias: Trade =
            serde_json::from_str(json_with_alias).expect("Failed to deserialize with alias");
        let trade_direct: Trade =
            serde_json::from_str(json_with_direct).expect("Failed to deserialize with direct key");

        assert_eq!(trade_alias.maker_address, trade_direct.maker_address);
    }

    #[test]
    fn test_trade_transaction_hash_alias() {
        // Test that "transaction_hash" key also works (in addition to "transactionHash" alias)
        let json_with_alias = r#"{"proxyWallet": "0x1111", "side": "sell", "asset": "ETH", "size": 20.0, "price": 0.8, "timestamp": 200, "transactionHash": "0xbbbb"}"#;
        let json_with_direct = r#"{"proxyWallet": "0x1111", "side": "sell", "asset": "ETH", "size": 20.0, "price": 0.8, "timestamp": 200, "transaction_hash": "0xbbbb"}"#;

        let trade_alias: Trade =
            serde_json::from_str(json_with_alias).expect("Failed to deserialize with alias");
        let trade_direct: Trade =
            serde_json::from_str(json_with_direct).expect("Failed to deserialize with direct key");

        assert_eq!(trade_alias.transaction_hash, trade_direct.transaction_hash);
    }
}
