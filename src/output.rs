use serde::Serialize;
use serde_json::Value;

use crate::cli::OutputMode;
use crate::engine::SearchResult;

#[derive(Serialize)]
pub struct Envelope {
    pub meta: Meta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits: Option<Vec<Hit>>,
}

#[derive(Serialize)]
pub struct Meta {
    pub total: usize,
    pub returned: usize,
    pub limit: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_searched: Option<usize>,
}

#[derive(Serialize, Clone)]
pub struct Hit {
    pub file: String,
    pub pointer: String,
    pub record: Value,
    pub score: f32,
}

pub fn format_output(
    results: &[SearchResult],
    total_matched: usize,
    limit: usize,
    output_mode: &OutputMode,
    bare: bool,
    count_only: bool,
    select_fields: &Option<Vec<String>>,
    files_searched: Option<usize>,
    max_bytes: Option<usize>,
) -> String {
    if count_only {
        if bare {
            return total_matched.to_string();
        }
        let envelope = Envelope {
            meta: Meta {
                total: total_matched,
                returned: 0,
                limit,
                truncated: false,
                files_searched,
            },
            results: None,
            hits: None,
        };
        return serde_json::to_string_pretty(&envelope).unwrap_or_default();
    }

    match output_mode {
        OutputMode::Match => {
            let all_objects: Vec<Value> = results
                .iter()
                .map(|r| project_fields(&r.record.value, select_fields))
                .collect();

            let (objects, byte_truncated) = truncate_to_budget(&all_objects, max_bytes);
            let truncated = total_matched > limit || byte_truncated;

            if bare {
                serde_json::to_string_pretty(&objects).unwrap_or_default()
            } else {
                let envelope = Envelope {
                    meta: Meta {
                        total: total_matched,
                        returned: objects.len(),
                        limit,
                        truncated,
                        files_searched,
                    },
                    results: Some(objects),
                    hits: None,
                };
                serde_json::to_string_pretty(&envelope).unwrap_or_default()
            }
        }
        OutputMode::Hit => {
            let all_hits: Vec<Hit> = results
                .iter()
                .map(|r| Hit {
                    file: r.record.file.clone(),
                    pointer: r.record.pointer.clone(),
                    record: project_fields(&r.record.value, select_fields),
                    score: r.score,
                })
                .collect();

            let (hits, byte_truncated) = truncate_to_budget(&all_hits, max_bytes);
            let truncated = total_matched > limit || byte_truncated;

            if bare {
                serde_json::to_string_pretty(&hits).unwrap_or_default()
            } else {
                let envelope = Envelope {
                    meta: Meta {
                        total: total_matched,
                        returned: hits.len(),
                        limit,
                        truncated,
                        files_searched,
                    },
                    results: None,
                    hits: Some(hits),
                };
                serde_json::to_string_pretty(&envelope).unwrap_or_default()
            }
        }
        OutputMode::Value => {
            let all_values: Vec<Value> = results
                .iter()
                .flat_map(|r| extract_matching_values(&r.record.value))
                .collect();

            let (values, byte_truncated) = truncate_to_budget(&all_values, max_bytes);
            let truncated = total_matched > limit || byte_truncated;

            if bare {
                serde_json::to_string_pretty(&values).unwrap_or_default()
            } else {
                let envelope = Envelope {
                    meta: Meta {
                        total: total_matched,
                        returned: values.len(),
                        limit,
                        truncated,
                        files_searched,
                    },
                    results: Some(values),
                    hits: None,
                };
                serde_json::to_string_pretty(&envelope).unwrap_or_default()
            }
        }
    }
}

/// Truncate a list of serializable items to fit within a byte budget.
/// Returns (kept_items, was_truncated).
/// Reserves ~200 bytes for the envelope/meta overhead.
fn truncate_to_budget<T: Serialize + Clone>(items: &[T], max_bytes: Option<usize>) -> (Vec<T>, bool) {
    let budget = match max_bytes {
        Some(b) => b,
        None => return (items.to_vec(), false),
    };

    let overhead = 200; // meta + envelope structure
    let available = budget.saturating_sub(overhead);
    let mut kept = Vec::new();
    let mut used: usize = 0;

    for item in items {
        let item_json = serde_json::to_string_pretty(item).unwrap_or_default();
        let item_bytes = item_json.len() + 2; // comma + newline
        if used + item_bytes > available && !kept.is_empty() {
            return (kept, true);
        }
        used += item_bytes;
        kept.push(item.clone());
    }

    (kept, false)
}

fn project_fields(value: &Value, select_fields: &Option<Vec<String>>) -> Value {
    match select_fields {
        Some(fields) => {
            if let Value::Object(map) = value {
                let filtered: serde_json::Map<String, Value> = map
                    .iter()
                    .filter(|(k, _)| fields.contains(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Value::Object(filtered)
            } else {
                value.clone()
            }
        }
        None => value.clone(),
    }
}

fn extract_matching_values(value: &Value) -> Vec<Value> {
    let mut values = Vec::new();
    match value {
        Value::Object(map) => {
            for val in map.values() {
                match val {
                    Value::String(_) | Value::Number(_) | Value::Bool(_) => {
                        values.push(val.clone());
                    }
                    _ => {}
                }
            }
        }
        _ => values.push(value.clone()),
    }
    values
}
