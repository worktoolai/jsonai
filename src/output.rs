use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;

use crate::cli::OutputMode;
use crate::engine::SearchResult;

pub fn to_json<T: Serialize>(value: &T, pretty: bool) -> String {
    if pretty {
        serde_json::to_string_pretty(value).unwrap_or_default()
    } else {
        serde_json::to_string(value).unwrap_or_default()
    }
}

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
    pretty: bool,
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
        return to_json(&envelope, pretty);
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
                to_json(&objects, pretty)
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
                to_json(&envelope, pretty)
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
                to_json(&hits, pretty)
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
                to_json(&envelope, pretty)
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
                to_json(&values, pretty)
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
                to_json(&envelope, pretty)
            }
        }
    }
}

/// Truncate a list of serializable items to fit within a byte budget.
/// Returns (kept_items, was_truncated).
/// Reserves ~200 bytes for the envelope/meta overhead.
fn truncate_to_budget<T: Serialize + Clone>(
    items: &[T],
    max_bytes: Option<usize>,
) -> (Vec<T>, bool) {
    let budget = match max_bytes {
        Some(b) => b,
        None => return (items.to_vec(), false),
    };

    let overhead = 200; // meta + envelope structure
    let available = budget.saturating_sub(overhead);
    let mut kept = Vec::new();
    let mut used: usize = 0;

    for item in items {
        let item_json = serde_json::to_string(item).unwrap_or_default();
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

// ---------------------------------------------------------------------------
// Overflow plan
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PlanEnvelope {
    pub meta: PlanMeta,
    pub plan: Plan,
    pub results: Vec<Value>,
}

#[derive(Serialize)]
pub struct PlanMeta {
    pub total: usize,
    pub returned: usize,
    pub overflow: bool,
    pub threshold: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_searched: Option<usize>,
}

#[derive(Serialize)]
pub struct Plan {
    pub fields: Vec<FieldInfo>,
    pub facets: BTreeMap<String, Vec<(String, usize)>>,
    pub commands: Vec<String>,
}

#[derive(Serialize)]
pub struct FieldInfo {
    pub name: String,
    pub path: String,
    pub distinct: usize,
}

fn escape_pointer_segment(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

/// Analyze matched records and produce a plan with fields, facets, and
/// suggested commands for narrowing down an overflow result set.
pub fn build_plan(results: &[SearchResult], query: &str, input: &str) -> Plan {
    // field_name -> (distinct values set, value -> count)
    let mut field_stats: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for sr in results {
        if let Value::Object(map) = &sr.record.value {
            for (key, val) in map {
                let entry = field_stats.entry(key.clone()).or_default();
                let stringified = value_to_facet_string(val);
                *entry.entry(stringified).or_insert(0) += 1;
            }
        }
    }

    // Build fields list sorted by distinct count ascending (most useful for
    // filtering first, i.e. lowest cardinality).
    let mut fields: Vec<FieldInfo> = field_stats
        .iter()
        .map(|(name, value_counts)| FieldInfo {
            name: name.clone(),
            path: format!("/{}", escape_pointer_segment(name)),
            distinct: value_counts.len(),
        })
        .collect();
    fields.sort_by_key(|f| f.distinct);

    // Build facets: only include fields with distinct count <= 20 (low
    // cardinality). Show top 5 values sorted by count descending.
    let mut facets: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    for (name, value_counts) in &field_stats {
        if value_counts.len() <= 20 {
            let mut pairs: Vec<(String, usize)> =
                value_counts.iter().map(|(v, &c)| (v.clone(), c)).collect();
            pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            pairs.truncate(5);
            facets.insert(name.clone(), pairs);
        }
    }

    // Generate command suggestions for each facet field.
    let commands: Vec<String> = facets
        .keys()
        .map(|field_name| {
            format!(
                "jsonai search -q {:?} --field {} {}",
                query, field_name, input
            )
        })
        .collect();

    Plan {
        fields,
        facets,
        commands,
    }
}

/// Format the full plan envelope as pretty-printed JSON.
pub fn format_plan_output(
    results: &[SearchResult],
    total_matched: usize,
    threshold: usize,
    files_searched: Option<usize>,
    query: &str,
    input: &str,
    pretty: bool,
) -> String {
    let plan = build_plan(results, query, input);

    let envelope = PlanEnvelope {
        meta: PlanMeta {
            total: total_matched,
            returned: 0,
            overflow: true,
            threshold,
            files_searched,
        },
        plan,
        results: vec![],
    };

    to_json(&envelope, pretty)
}

/// Convert a serde_json::Value to a string suitable for facet counting.
fn value_to_facet_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        // For arrays/objects, use compact JSON so distinct counting still works.
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_field_path_escapes_pointer_segment() {
        let results = vec![SearchResult {
            record: crate::engine::Record {
                file: "f.json".to_string(),
                pointer: "".to_string(),
                value: serde_json::json!({
                    "src/lib": 1,
                    "config~backup": 2
                }),
            },
            score: 1.0,
        }];

        let plan = build_plan(&results, "q", "input.json");
        let mut paths: Vec<String> = plan.fields.into_iter().map(|f| f.path).collect();
        paths.sort();

        assert!(paths.contains(&"/src~1lib".to_string()));
        assert!(paths.contains(&"/config~0backup".to_string()));
    }
}
