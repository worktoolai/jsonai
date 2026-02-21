mod cli;
mod engine;
mod manipulate;
mod output;

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
    };

    std::process::exit(exit_code);
}

fn run_cat(args: cli::CatArgs, pretty: bool) -> Result<()> {
    let value = load_json_value(&args.input)?;

    let output_value = match &args.pointer {
        Some(ptr) => {
            let resolved = value.pointer(ptr)
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

    let mut results = engine.search(
        &args.query,
        &fields,
        &args.r#match,
        search_limit,
        0,
    )?;

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
    let mut all_records = Vec::new();
    let mut file_count = 0;

    for entry in glob::glob(pattern).context("Invalid glob pattern")? {
        let path = entry.context("Failed to read glob entry")?;
        if path.is_file() {
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
    }

    if file_count == 0 {
        bail!("No JSON files found matching pattern: {}", pattern);
    }

    Ok((all_records, file_count))
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
