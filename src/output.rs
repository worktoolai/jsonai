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

#[derive(Serialize)]
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
            let objects: Vec<Value> = results
                .iter()
                .map(|r| project_fields(&r.record.value, select_fields))
                .collect();

            if bare {
                serde_json::to_string_pretty(&objects).unwrap_or_default()
            } else {
                let envelope = Envelope {
                    meta: Meta {
                        total: total_matched,
                        returned: objects.len(),
                        limit,
                        truncated: total_matched > limit,
                        files_searched,
                    },
                    results: Some(objects),
                    hits: None,
                };
                serde_json::to_string_pretty(&envelope).unwrap_or_default()
            }
        }
        OutputMode::Hit => {
            let hits: Vec<Hit> = results
                .iter()
                .map(|r| Hit {
                    file: r.record.file.clone(),
                    pointer: r.record.pointer.clone(),
                    record: project_fields(&r.record.value, select_fields),
                    score: r.score,
                })
                .collect();

            if bare {
                serde_json::to_string_pretty(&hits).unwrap_or_default()
            } else {
                let envelope = Envelope {
                    meta: Meta {
                        total: total_matched,
                        returned: hits.len(),
                        limit,
                        truncated: total_matched > limit,
                        files_searched,
                    },
                    results: None,
                    hits: Some(hits),
                };
                serde_json::to_string_pretty(&envelope).unwrap_or_default()
            }
        }
        OutputMode::Value => {
            let values: Vec<Value> = results
                .iter()
                .flat_map(|r| extract_matching_values(&r.record.value))
                .collect();

            if bare {
                serde_json::to_string_pretty(&values).unwrap_or_default()
            } else {
                let envelope = Envelope {
                    meta: Meta {
                        total: total_matched,
                        returned: values.len(),
                        limit,
                        truncated: total_matched > limit,
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
