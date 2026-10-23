/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde_json::Value as Json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTotal {
    Exact(u64),
    AtLeast(u64),
    Unknown,
}

impl HitTotal {
    pub fn parse(total: Option<&Json>) -> Self {
        match total {
            Some(Json::Number(value)) => value.as_u64().map_or(Self::Unknown, Self::Exact),
            Some(Json::Object(total)) => {
                let Some(value) = total.get("value").and_then(Json::as_u64) else {
                    return Self::Unknown;
                };
                match total.get("relation").and_then(Json::as_str) {
                    Some("gte") => Self::AtLeast(value),
                    _ => Self::Exact(value),
                }
            }
            _ => Self::Unknown,
        }
    }

    pub fn exact(self) -> Option<u64> {
        match self {
            Self::Exact(value) => Some(value),
            Self::AtLeast(_) | Self::Unknown => None,
        }
    }

    pub fn exceeds(self, seen: u64) -> bool {
        match self {
            Self::Exact(value) => value > seen,
            Self::AtLeast(value) => value >= seen,
            Self::Unknown => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHits<'a> {
    pub total: HitTotal,
    pub hits: &'a [Json],
}

impl<'a> SearchHits<'a> {
    pub fn from_response(response: &'a Json) -> Option<Self> {
        let hits = response.get("hits")?;
        Some(Self {
            total: HitTotal::parse(hits.get("total")),
            hits: hits.get("hits")?.as_array()?.as_slice(),
        })
    }

    pub fn last_sort(&self) -> Option<&'a Json> {
        self.hits.last()?.get("sort")
    }
}

pub fn hit_body(hit: &Json) -> Json {
    hit.get("_source")
        .or_else(|| hit.get("fields"))
        .cloned()
        .unwrap_or_else(|| Json::Object(Default::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn totals_respect_the_relation() {
        assert_eq!(
            HitTotal::parse(Some(&json!({"value": 3, "relation": "eq"}))),
            HitTotal::Exact(3)
        );
        let bounded = HitTotal::parse(Some(&json!({"value": 10000, "relation": "gte"})));
        assert_eq!(bounded.exact(), None);
        assert!(bounded.exceeds(10_000));
        assert_eq!(HitTotal::parse(Some(&json!(7))), HitTotal::Exact(7));
        assert_eq!(HitTotal::parse(None), HitTotal::Unknown);
        assert!(!HitTotal::Exact(5).exceeds(5));
    }

    #[test]
    fn hits_are_read_from_search_responses() {
        let response = json!({"hits": {"total": {"value": 2, "relation": "eq"}, "hits": [
            {"_id": "1", "_source": {"a": 1}, "sort": [1, 0]},
            {"_id": "2", "fields": {"b": [2]}, "sort": [2, 5]}
        ]}});
        let hits = SearchHits::from_response(&response).unwrap();
        assert_eq!(hits.hits.len(), 2);
        assert_eq!(hits.last_sort(), Some(&json!([2, 5])));
        assert_eq!(hit_body(&hits.hits[0]), json!({"a": 1}));
        assert_eq!(hit_body(&hits.hits[1]), json!({"b": [2]}));
        assert!(SearchHits::from_response(&json!({"acknowledged": true})).is_none());
    }
}
