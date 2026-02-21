use anyhow::{bail, Context, Result};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, RcIter};
use jaq_json::Val;
use serde_json::Value;
use std::io::{self, Read};

use crate::output;

pub fn run_query(filter_str: &str, input: &str, pretty: bool) -> Result<()> {
    let value = load_input(input)?;
    let results = eval(filter_str, value)?;

    match results.len() {
        0 => {}
        1 => println!("{}", output::to_json(&results[0], pretty)),
        _ => println!("{}", output::to_json(&results, pretty)),
    }

    Ok(())
}

fn load_input(input: &str) -> Result<Value> {
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

fn eval(filter_str: &str, input: Value) -> Result<Vec<Value>> {
    let loader = Loader::new(jaq_std::defs().chain(jaq_json::defs()));
    let arena = Arena::default();

    let program = File {
        code: filter_str,
        path: (),
    };
    let modules = loader
        .load(&arena, program)
        .map_err(|errs| anyhow::anyhow!("Parse error: {:?}", errs))?;

    let filter = Compiler::default()
        .with_funs(jaq_std::funs().chain(jaq_json::funs()))
        .compile(modules)
        .map_err(|errs| anyhow::anyhow!("Compile error: {:?}", errs))?;

    let inputs = RcIter::new(core::iter::empty());
    let ctx = Ctx::new([], &inputs);
    let out = filter.run((ctx, Val::from(input)));

    let mut results = Vec::new();
    for item in out {
        match item {
            Ok(val) => results.push(Value::from(val)),
            Err(e) => bail!("Runtime error: {e}"),
        }
    }

    Ok(results)
}
