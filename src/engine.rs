use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use tantivy::collector::TopDocs;
use tantivy::query::{FuzzyTermQuery, QueryParser, RegexQuery};
use tantivy::schema::{self, *};
use tantivy::{Index, ReloadPolicy, TantivyDocument, Term};

use crate::cli::MatchMode;

/// A record extracted from a JSON file
#[derive(Debug, Clone)]
pub struct Record {
    pub pointer: String,
    pub file: String,
    pub value: Value,
}

/// Search result with score
#[derive(Debug)]
pub struct SearchResult {
    pub record: Record,
    pub score: f32,
}

/// The search engine
pub struct Engine {
    index: Index,
    #[allow(dead_code)]
    schema: Schema,
    content_field: Field,
    all_text_field: Field,
    pointer_field: Field,
    file_field: Field,
    source_field: Field,
}

impl Engine {
    pub fn new() -> Result<Self> {
        let mut builder = Schema::builder();

        let json_options = JsonObjectOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();

        let content_field = builder.add_json_field("content", json_options);
        let all_text_field = builder.add_text_field("_all", TEXT | STORED);
        let pointer_field = builder.add_text_field("_pointer", STRING | STORED);
        let file_field = builder.add_text_field("_file", STRING | STORED);
        let source_field = builder.add_text_field("_source", STORED);

        let schema = builder.build();
        let index = Index::create_in_ram(schema.clone());

        Ok(Engine {
            index,
            schema,
            content_field,
            all_text_field,
            pointer_field,
            file_field,
            source_field,
        })
    }

    pub fn index_records(&self, records: &[Record]) -> Result<()> {
        let mut writer = self
            .index
            .writer(50_000_000)
            .context("Failed to create index writer")?;

        for record in records {
            let all_text = collect_all_text(&record.value);
            let source_json = serde_json::to_string(&record.value)?;

            let json_object: BTreeMap<String, schema::OwnedValue> = match &record.value {
                Value::Object(map) => map
                    .iter()
                    .map(|(k, v)| (k.clone(), schema::OwnedValue::from(v.clone())))
                    .collect(),
                _ => {
                    let mut m = BTreeMap::new();
                    m.insert("_value".to_string(), schema::OwnedValue::from(record.value.clone()));
                    m
                }
            };

            let mut doc = TantivyDocument::default();
            doc.add_object(self.content_field, json_object);
            doc.add_text(self.all_text_field, &all_text);
            doc.add_text(self.pointer_field, &record.pointer);
            doc.add_text(self.file_field, &record.file);
            doc.add_text(self.source_field, &source_json);

            writer.add_document(doc)?;
        }

        writer.commit().context("Failed to commit index")?;
        Ok(())
    }

    pub fn search(
        &self,
        query_str: &str,
        fields: &[String],
        match_mode: &MatchMode,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchResult>> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .context("Failed to create reader")?;

        let searcher = reader.searcher();

        let query: Box<dyn tantivy::query::Query> = match match_mode {
            MatchMode::Text | MatchMode::Exact => {
                let search_fields = if fields.is_empty() {
                    vec![self.all_text_field]
                } else {
                    vec![self.content_field]
                };

                let mut parser = QueryParser::for_index(&self.index, search_fields);
                parser.set_conjunction_by_default();

                let effective_query = if !fields.is_empty() {
                    fields
                        .iter()
                        .map(|f| format!("content.{}:{}", f, query_str))
                        .collect::<Vec<_>>()
                        .join(" OR ")
                } else {
                    query_str.to_string()
                };

                parser
                    .parse_query(&effective_query)
                    .context("Failed to parse query")?
            }
            MatchMode::Fuzzy => {
                let term = if fields.is_empty() {
                    Term::from_field_text(self.all_text_field, &query_str.to_lowercase())
                } else {
                    Term::from_field_text(self.all_text_field, &query_str.to_lowercase())
                };

                Box::new(FuzzyTermQuery::new(term, 2, true))
            }
            MatchMode::Regex => {
                let field = if fields.is_empty() {
                    self.all_text_field
                } else {
                    self.all_text_field
                };

                Box::new(
                    RegexQuery::from_pattern(query_str, field)
                        .context("Failed to parse regex")?,
                )
            }
        };

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit + offset))
            .context("Search failed")?;

        let mut results = Vec::new();
        for (i, (score, doc_address)) in top_docs.into_iter().enumerate() {
            if i < offset {
                continue;
            }

            let doc: TantivyDocument = searcher.doc(doc_address)?;

            let pointer = get_stored_text(&doc, self.pointer_field);
            let file = get_stored_text(&doc, self.file_field);
            let source = get_stored_text(&doc, self.source_field);

            let value: Value = serde_json::from_str(&source).unwrap_or(Value::Null);

            results.push(SearchResult {
                record: Record {
                    pointer,
                    file,
                    value,
                },
                score,
            });
        }

        Ok(results)
    }
}

fn get_stored_text(doc: &TantivyDocument, field: Field) -> String {
    use tantivy::schema::Value as TValue;
    doc.get_first(field)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Recursively collect all string values from a JSON value
fn collect_all_text(value: &Value) -> String {
    let mut texts = Vec::new();
    collect_text_recursive(value, &mut texts);
    texts.join(" ")
}

fn collect_text_recursive(value: &Value, texts: &mut Vec<String>) {
    match value {
        Value::String(s) => texts.push(s.clone()),
        Value::Number(n) => texts.push(n.to_string()),
        Value::Bool(b) => texts.push(b.to_string()),
        Value::Array(arr) => {
            for item in arr {
                collect_text_recursive(item, texts);
            }
        }
        Value::Object(map) => {
            for val in map.values() {
                collect_text_recursive(val, texts);
            }
        }
        Value::Null => {}
    }
}

/// Extract records from a JSON value, walking the tree
pub fn extract_records(value: &Value, file: &str) -> Vec<Record> {
    let mut records = Vec::new();
    extract_recursive(value, "", file, &mut records);
    records
}

fn extract_recursive(value: &Value, pointer: &str, file: &str, records: &mut Vec<Record>) {
    match value {
        Value::Object(map) => {
            records.push(Record {
                pointer: if pointer.is_empty() {
                    "/".to_string()
                } else {
                    pointer.to_string()
                },
                file: file.to_string(),
                value: value.clone(),
            });

            for (key, val) in map {
                let child_pointer = format!("{}/{}", pointer, key);
                extract_recursive(val, &child_pointer, file, records);
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                let child_pointer = format!("{}/{}", pointer, i);
                extract_recursive(item, &child_pointer, file, records);
            }
        }
        _ => {}
    }
}

/// Deduplicate results: if a child matches, remove its ancestor
pub fn dedup_results(results: &mut Vec<SearchResult>) {
    let pointers: Vec<(String, String)> = results
        .iter()
        .map(|r| (r.record.pointer.clone(), r.record.file.clone()))
        .collect();

    results.retain(|r| {
        let my_pointer = &r.record.pointer;
        let my_file = &r.record.file;
        !pointers.iter().any(|(other_ptr, other_file)| {
            other_ptr != my_pointer
                && other_file == my_file
                && other_ptr.starts_with(my_pointer.as_str())
                && other_ptr.len() > my_pointer.len()
        })
    });
}
