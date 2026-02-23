mod cli;
mod engine;
mod manipulate;
mod output;
mod query;

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde_json::Value;
use std::io::{self, Read};
use std::path::Path;

use cli::{Cli, Commands, SearchArgs};
use engine::{dedup_results, extract_records, Engine, Record};
use output::{format_output, format_plan_output};

fn main() {
    let cli = Cli::parse();

    // stdout (search/fields): compact by default, --pretty to opt-in
    let stdout_pretty = cli.pretty;
    // file writes (set/add/delete/patch): pretty by default, --compact to opt-out
    let file_pretty = !cli.compact;

    let exit_code = match cli.command {
        Commands::Cat(args) => match run_cat(args, stdout_pretty) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
        Commands::Search(args) => match run_search(args, stdout_pretty) {
            Ok(has_matches) => {
                if has_matches {
                    0
                } else {
                    1
                }
            }
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
        Commands::Fields(args) => match run_fields(args, stdout_pretty) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
        Commands::Set(args) => match manipulate::json_set(
            &args.file,
            &args.pointer,
            &args.value,
            args.output.as_deref(),
            args.dry_run,
            file_pretty,
        ) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
        Commands::Add(args) => match manipulate::json_add(
            &args.file,
            &args.pointer,
            &args.value,
            args.output.as_deref(),
            args.dry_run,
            file_pretty,
        ) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
        Commands::Delete(args) => match manipulate::json_delete(
            &args.file,
            &args.pointer,
            args.output.as_deref(),
            args.dry_run,
            file_pretty,
        ) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
        Commands::Patch(args) => match manipulate::json_patch(
            &args.file,
            args.patch.as_deref(),
            args.output.as_deref(),
            args.dry_run,
            file_pretty,
        ) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
        Commands::Query(args) => match query::run_query(&args.filter, &args.input, stdout_pretty) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {:#}", e);
                2
            }
        },
    };

    std::process::exit(exit_code);
}

fn run_cat(args: cli::CatArgs, pretty: bool) -> Result<()> {
    let value = load_json_value(&args.input)?;

    let output_value = match &args.pointer {
        Some(ptr) => {
            let resolved = value
                .pointer(ptr)
                .with_context(|| format!("Pointer {} not found", ptr))?;
            resolved.clone()
        }
        None => value,
    };

    let output = output::to_json(&output_value, pretty);
    println!("{}", output);
    Ok(())
}

fn load_json_value(input: &str) -> Result<Value> {
    if input == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read stdin")?;
        serde_json::from_str(&buf).context("Invalid JSON from stdin")
    } else {
        let content =
            std::fs::read_to_string(input).with_context(|| format!("Failed to read {}", input))?;
        serde_json::from_str(&content).with_context(|| format!("Invalid JSON in {}", input))
    }
}

fn run_search(args: SearchArgs, pretty: bool) -> Result<bool> {
    let (records, files_searched) = load_records(&args.input)?;

    if records.is_empty() {
        bail!("No JSON objects found in input");
    }

    let engine = Engine::new()?;
    engine.index_records(&records)?;

    let fields = if !args.field.is_empty() {
        args.field.clone()
    } else {
        vec![]
    };

    // When plan mode is possible, fetch more results so facets are accurate
    let search_limit = if args.plan || !args.no_overflow {
        std::cmp::max(args.limit + args.offset, args.threshold * 2)
    } else {
        args.limit + args.offset
    };

    let mut results = engine.search(&args.query, &fields, &args.r#match, search_limit, 0)?;

    dedup_results(&mut results);

    let total_matched = results.len();

    // Overflow detection: plan mode forced, or results exceed threshold
    let overflow = args.plan || (!args.no_overflow && total_matched > args.threshold);
    if overflow {
        let output = format_plan_output(
            &results,
            total_matched,
            args.threshold,
            Some(files_searched),
            &args.query,
            &args.input,
            pretty,
        );
        println!("{}", output);
        return Ok(true);
    }

    if args.offset > 0 && args.offset < results.len() {
        results = results.into_iter().skip(args.offset).collect();
    }

    if results.len() > args.limit {
        results.truncate(args.limit);
    }

    let select_fields = args
        .select
        .as_ref()
        .map(|s| s.split(',').map(|f| f.trim().to_string()).collect());

    let output = format_output(
        &results,
        total_matched,
        args.limit,
        &args.output,
        args.bare,
        args.count_only,
        &select_fields,
        Some(files_searched),
        args.max_bytes,
        pretty,
    );

    println!("{}", output);

    Ok(total_matched > 0)
}

fn load_records(input: &str) -> Result<(Vec<Record>, usize)> {
    if input == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read stdin")?;
        let value: Value = serde_json::from_str(&buf).context("Invalid JSON from stdin")?;
        let records = extract_records(&value, "stdin");
        Ok((records, 1))
    } else {
        let path = Path::new(input);

        if path.is_file() {
            let records = load_file(input)?;
            Ok((records, 1))
        } else if path.is_dir() {
            load_directory(input)
        } else {
            load_glob(input)
        }
    }
}

fn load_file(path: &str) -> Result<Vec<Record>> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    let value: Value =
        serde_json::from_str(&content).with_context(|| format!("Invalid JSON in {}", path))?;
    Ok(extract_records(&value, path))
}

fn load_directory(dir: &str) -> Result<(Vec<Record>, usize)> {
    let pattern = format!("{}/**/*.json", dir);
    load_glob(&pattern)
}

fn load_glob(pattern: &str) -> Result<(Vec<Record>, usize)> {
    let matcher = glob::Pattern::new(pattern).context("Invalid glob pattern")?;
    let search_root = glob_search_root(pattern);
    let walk_root = glob_walk_root(&search_root);

    let mut all_records = Vec::new();
    let mut file_count = 0;

    for path in walk_files_respecting_gitignore(&walk_root)? {
        if !path_matches_glob(&matcher, &path) {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();
        match load_file(&path_str) {
            Ok(records) => {
                all_records.extend(records);
                file_count += 1;
            }
            Err(e) => {
                eprintln!("Warning: skipping {}: {}", path_str, e);
            }
        }
    }

    if file_count == 0 {
        bail!("No JSON files found matching pattern: {}", pattern);
    }

    Ok((all_records, file_count))
}

fn walk_files_respecting_gitignore(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();

    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .require_git(false);

    for entry in builder.build() {
        let entry = entry.with_context(|| format!("Failed to walk {}", root.display()))?;
        let path = entry.path();

        if path_has_ignored_runtime_dir(path) {
            continue;
        }

        if entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            files.push(entry.into_path());
        }
    }

    Ok(files)
}

const RUNTIME_IGNORED_DIRS: &[&str] = &[".worktoolai"];

fn path_has_ignored_runtime_dir(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .map(|name| RUNTIME_IGNORED_DIRS.contains(&name))
            .unwrap_or(false)
    })
}

fn glob_search_root(pattern: &str) -> std::path::PathBuf {
    let wildcard_start = pattern
        .char_indices()
        .find(|(_, c)| matches!(c, '*' | '?' | '[' | '{'))
        .map(|(idx, _)| idx);

    let prefix = wildcard_start.map(|idx| &pattern[..idx]).unwrap_or(pattern);
    if prefix.is_empty() {
        return ".".into();
    }

    let prefix_path = Path::new(prefix);
    let root = if prefix.ends_with('/') || prefix.ends_with(std::path::MAIN_SEPARATOR) {
        prefix_path
    } else {
        prefix_path.parent().unwrap_or_else(|| Path::new("."))
    };

    if root.as_os_str().is_empty() {
        ".".into()
    } else {
        root.to_path_buf()
    }
}

fn glob_walk_root(search_root: &Path) -> std::path::PathBuf {
    let mut candidate = search_root;

    while let Some(parent) = candidate.parent() {
        if parent == candidate {
            break;
        }
        if parent.join(".git").exists() {
            candidate = parent;
            continue;
        }
        break;
    }

    candidate.to_path_buf()
}

fn path_matches_glob(matcher: &glob::Pattern, path: &Path) -> bool {
    if matcher.matches_path(path) {
        return true;
    }

    if let Ok(current_dir) = std::env::current_dir() {
        if let Ok(relative_path) = path.strip_prefix(&current_dir) {
            if matcher.matches_path(relative_path) {
                return true;
            }

            let dot_relative = Path::new(".").join(relative_path);
            if matcher.matches_path(&dot_relative) {
                return true;
            }
        }
    }

    false
}

fn run_fields(args: cli::FieldsArgs, pretty: bool) -> Result<()> {
    let content = std::fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read {}", args.input))?;
    let value: Value = serde_json::from_str(&content)
        .with_context(|| format!("Invalid JSON in {}", args.input))?;

    let mut fields = Vec::new();
    collect_field_paths(&value, "", &mut fields);
    fields.sort();
    fields.dedup();

    let output = output::to_json(&fields, pretty);
    println!("{}", output);

    Ok(())
}

fn collect_field_paths(value: &Value, prefix: &str, fields: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                fields.push(path.clone());
                collect_field_paths(val, &path, fields);
            }
        }
        Value::Array(arr) => {
            if let Some(first) = arr.first() {
                collect_field_paths(first, prefix, fields);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{load_directory, load_glob};
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_json(path: &Path, value: serde_json::Value) {
        fs::write(path, serde_json::to_string(&value).unwrap()).unwrap();
    }

    #[test]
    fn load_directory_respects_gitignore() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join(".gitignore"), "target/\n").unwrap();

        fs::create_dir(temp.path().join("target")).unwrap();
        write_json(
            &temp.path().join("target/ignored.json"),
            json!({ "msg": "ignored" }),
        );
        write_json(&temp.path().join("keep.json"), json!({ "msg": "kept" }));

        let (records, file_count) = load_directory(temp.path().to_str().unwrap()).unwrap();

        assert_eq!(file_count, 1);
        assert!(records
            .iter()
            .all(|r| !r.file.ends_with("target/ignored.json")));
        assert!(records.iter().any(|r| r.file.ends_with("keep.json")));
    }

    #[test]
    fn load_glob_respects_gitignore() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join(".gitignore"), "target/\n").unwrap();

        fs::create_dir(temp.path().join("target")).unwrap();
        write_json(
            &temp.path().join("target/ignored.json"),
            json!({ "msg": "ignored" }),
        );
        write_json(&temp.path().join("keep.json"), json!({ "msg": "kept" }));

        let pattern = format!("{}/**/*.json", temp.path().display());
        let (records, file_count) = load_glob(&pattern).unwrap();

        assert_eq!(file_count, 1);
        assert!(records
            .iter()
            .all(|r| !r.file.ends_with("target/ignored.json")));
        assert!(records.iter().any(|r| r.file.ends_with("keep.json")));
    }

    #[test]
    fn load_directory_ignores_worktoolai_dir() {
        let temp = tempdir().unwrap();

        fs::create_dir(temp.path().join(".worktoolai")).unwrap();
        write_json(
            &temp.path().join(".worktoolai/ignored.json"),
            json!({ "msg": "ignored" }),
        );
        write_json(&temp.path().join("keep.json"), json!({ "msg": "kept" }));

        let (records, file_count) = load_directory(temp.path().to_str().unwrap()).unwrap();

        assert_eq!(file_count, 1);
        assert!(records
            .iter()
            .all(|r| !r.file.contains("/.worktoolai/") && !r.file.ends_with("/.worktoolai")));
        assert!(records.iter().any(|r| r.file.ends_with("keep.json")));
    }

    #[test]
    fn load_glob_ignores_worktoolai_dir() {
        let temp = tempdir().unwrap();

        fs::create_dir(temp.path().join(".worktoolai")).unwrap();
        write_json(
            &temp.path().join(".worktoolai/ignored.json"),
            json!({ "msg": "ignored" }),
        );
        write_json(&temp.path().join("keep.json"), json!({ "msg": "kept" }));

        let pattern = format!("{}/**/*.json", temp.path().display());
        let (records, file_count) = load_glob(&pattern).unwrap();

        assert_eq!(file_count, 1);
        assert!(records
            .iter()
            .all(|r| !r.file.contains("/.worktoolai/") && !r.file.ends_with("/.worktoolai")));
        assert!(records.iter().any(|r| r.file.ends_with("keep.json")));
    }
}
