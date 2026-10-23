/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::SearchClient;
use crate::paths::segment;
use relix_driver_api::DriverError;
use relix_driver_http::Method;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

pub const DEFAULT_RESULT_WINDOW: u64 = 10_000;
const SETTING: &str = "index.max_result_window";

#[derive(Default)]
pub struct ResultWindows {
    known: Mutex<HashMap<String, u64>>,
}

impl ResultWindows {
    pub async fn get(&self, client: &SearchClient, collection: &str) -> Result<u64, DriverError> {
        if let Some(window) = self.cached(collection) {
            return Ok(window);
        }
        let path = format!(
            "/{}/_settings/{SETTING}?include_defaults=true&flat_settings=true&expand_wildcards=all",
            segment(collection)?
        );
        let settings = client.json(Method::GET, &path, None).await?;
        let window = smallest_window(&settings);
        self.known
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(collection.to_owned(), window);
        Ok(window)
    }

    fn cached(&self, collection: &str) -> Option<u64> {
        self.known
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(collection)
            .copied()
    }
}

fn smallest_window(settings: &Json) -> u64 {
    settings
        .as_object()
        .into_iter()
        .flat_map(|indices| indices.values())
        .filter_map(|index| {
            let value = index["settings"]
                .get(SETTING)
                .or_else(|| index["defaults"].get(SETTING))?;
            match value {
                Json::String(text) => text.parse().ok(),
                other => other.as_u64(),
            }
        })
        .min()
        .unwrap_or(DEFAULT_RESULT_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_smallest_member_window_wins() {
        let settings = json!({
            "a": {"settings": {}, "defaults": {"index.max_result_window": "10000"}},
            "b": {"settings": {"index.max_result_window": "500"}, "defaults": {}}
        });
        assert_eq!(smallest_window(&settings), 500);
        assert_eq!(smallest_window(&json!({})), DEFAULT_RESULT_WINDOW);
    }
}
