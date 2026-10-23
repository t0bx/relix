/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Position of the next page. Pages inside `index.max_result_window` use `from`; past
/// the window the token carries the sort values of the last hit for `search_after`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PageToken {
    Offset { from: u64 },
    After { from: u64, values: Vec<Json> },
}

impl Default for PageToken {
    fn default() -> Self {
        Self::Offset { from: 0 }
    }
}

impl PageToken {
    pub fn decode(token: Option<&str>) -> Result<Self, DriverError> {
        match token.map(str::trim).filter(|token| !token.is_empty()) {
            None => Ok(Self::default()),
            Some(token) => serde_json::from_str(token)
                .map_err(|_| DriverError::query("invalid page token, reload the first page")),
        }
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from(&self) -> u64 {
        match self {
            Self::Offset { from } | Self::After { from, .. } => *from,
        }
    }

    pub fn next(from: u64, limit: u64, window: u64, last_sort: Option<&Json>) -> Option<Self> {
        if from + limit <= window {
            return Some(Self::Offset { from });
        }
        let values = last_sort?.as_array()?.clone();
        Some(Self::After { from, values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tokens_round_trip() {
        let token = PageToken::After {
            from: 10_000,
            values: vec![json!(1.5), json!(42)],
        };
        assert_eq!(PageToken::decode(Some(&token.encode())).unwrap(), token);
        assert_eq!(PageToken::decode(None).unwrap(), PageToken::default());
        assert!(PageToken::decode(Some("garbage")).is_err());
    }

    #[test]
    fn switches_to_search_after_past_the_window() {
        let sort = json!(["b", 7]);
        assert_eq!(
            PageToken::next(9_950, 50, 10_000, Some(&sort)),
            Some(PageToken::Offset { from: 9_950 })
        );
        assert_eq!(
            PageToken::next(9_960, 50, 10_000, Some(&sort)),
            Some(PageToken::After {
                from: 9_960,
                values: vec![json!("b"), json!(7)]
            })
        );
        assert_eq!(PageToken::next(9_960, 50, 10_000, None), None);
    }
}
