use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub id: String,
    pub question: String,
    pub condition_id: String,
    pub active: bool,
    #[serde(default)]
    pub clob_token_ids: Option<String>,
    #[serde(default)]
    pub outcomes: Option<String>,
}

impl Market {
    /// Helper function to parse the clob_token_ids field from a string into a vector of strings.
    pub fn parsed_clob_token_ids(&self) -> Vec<String> {
        if let Some(ref s) = self.clob_token_ids {
            serde_json::from_str(s).unwrap_or_default()
        } else {
            vec![]
        }
    }

    /// Helper function to parse the outcomes field from a string into a vector of strings.
    pub fn parsed_outcomes(&self) -> Vec<String> {
        if let Some(ref s) = self.outcomes {
            serde_json::from_str(s).unwrap_or_default()
        } else {
            vec![]
        }
    }
}
