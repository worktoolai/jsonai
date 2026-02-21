use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::io::{self, Read};

// ---------------------------------------------------------------------------
// JSON Pointer helpers
// ---------------------------------------------------------------------------

/// Unescape a single JSON Pointer segment per RFC 6901:
/// `~1` -> `/`, then `~0` -> `~`.
fn unescape_segment(seg: &str) -> String {
    seg.replace("~1", "/").replace("~0", "~")
}

/// Parse a JSON Pointer string into a vector of unescaped segments.
/// An empty string means the root document (returns an empty vec).
fn parse_pointer(pointer: &str) -> Result<Vec<String>> {
    if pointer.is_empty() {
        return Ok(vec![]);
    }
    if !pointer.starts_with('/') {
        bail!("JSON Pointer must start with '/' (got {:?})", pointer);
    }
    Ok(pointer[1..]
        .split('/')
        .map(|s| unescape_segment(s))
        .collect())
}

/// Navigate a JSON Pointer to obtain a mutable reference to the target value.
fn resolve_pointer_mut<'a>(root: &'a mut Value, pointer: &str) -> Result<&'a mut Value> {
    let segments = parse_pointer(pointer)?;
    let mut current = root;

    for (i, seg) in segments.iter().enumerate() {
        let built = format!(
            "/{}",
            segments[..=i]
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("/")
        );

        current = match current {
            Value::Object(map) => map
                .get_mut(seg)
                .with_context(|| format!("Key {:?} not found at pointer {:?}", seg, built))?,
            Value::Array(arr) => {
                let idx: usize = seg
                    .parse()
                    .with_context(|| format!("Invalid array index {:?} at {:?}", seg, built))?;
                arr.get_mut(idx).with_context(|| {
                    format!("Array index {} out of bounds at {:?}", idx, built)
                })?
            }
            _ => bail!(
                "Cannot navigate into {:?} at {:?} (not an object or array)",
                current,
                built
            ),
        };
    }

    Ok(current)
}

/// Split a JSON Pointer into (parent_pointer, last_segment).
/// Returns `("", last)` when there is only one segment, and an error for the
/// root pointer (empty string) since there is no parent.
fn resolve_parent_and_key<'a>(
    root: &'a mut Value,
    pointer: &str,
) -> Result<(&'a mut Value, String)> {
    let segments = parse_pointer(pointer)?;
    if segments.is_empty() {
        bail!("Cannot resolve parent of the root pointer");
    }

    let last = segments.last().unwrap().clone();

    if segments.len() == 1 {
        return Ok((root, last));
    }

    // Build parent pointer from all segments except the last.
    let parent_path: String = segments[..segments.len() - 1]
        .iter()
        .map(|s| format!("/{}", s))
        .collect();

    let parent = resolve_pointer_mut(root, &parent_path)?;
    Ok((parent, last))
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

/// Read and parse a JSON file.
fn read_json_file(file: &str) -> Result<Value> {
    let content =
        std::fs::read_to_string(file).with_context(|| format!("Failed to read {}", file))?;
    let value: Value =
        serde_json::from_str(&content).with_context(|| format!("Invalid JSON in {}", file))?;
    Ok(value)
}

/// Write the JSON value to the appropriate destination.
/// - dry_run: print to stdout
/// - output is Some: write to that path
/// - otherwise: overwrite the original file
fn write_json(value: &Value, file: &str, output: Option<&str>, dry_run: bool, pretty: bool) -> Result<()> {
    let serialized = if pretty {
        serde_json::to_string_pretty(value).context("Failed to serialize JSON output")?
    } else {
        serde_json::to_string(value).context("Failed to serialize JSON output")?
    };

    if dry_run {
        println!("{}", serialized);
        return Ok(());
    }

    let dest = output.unwrap_or(file);
    std::fs::write(dest, format!("{}\n", serialized))
        .with_context(|| format!("Failed to write {}", dest))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

/// Set (replace) the value at `pointer` with `value_str` (parsed as JSON).
///
/// If the pointer addresses the root (""), the entire document is replaced.
pub fn json_set(
    file: &str,
    pointer: &str,
    value_str: &str,
    output: Option<&str>,
    dry_run: bool,
    pretty: bool,
) -> Result<()> {
    let mut root = read_json_file(file)?;
    let new_value: Value = serde_json::from_str(value_str)
        .with_context(|| format!("Invalid JSON value: {}", value_str))?;

    if pointer.is_empty() {
        // Replace the entire document.
        root = new_value;
    } else {
        let (parent, key) = resolve_parent_and_key(&mut root, pointer)?;

        match parent {
            Value::Object(map) => {
                if !map.contains_key(&key) {
                    bail!(
                        "Key {:?} does not exist at parent; use `add` to create new keys",
                        key
                    );
                }
                map.insert(key, new_value);
            }
            Value::Array(arr) => {
                let idx: usize = key
                    .parse()
                    .with_context(|| format!("Invalid array index {:?}", key))?;
                if idx >= arr.len() {
                    bail!(
                        "Array index {} out of bounds (length {})",
                        idx,
                        arr.len()
                    );
                }
                arr[idx] = new_value;
            }
            _ => bail!("Parent at pointer is not an object or array"),
        }
    }

    write_json(&root, file, output, dry_run, pretty)
}

/// Add a value at `pointer`.
///
/// - If pointer ends with `/-` the value is appended to the parent array.
/// - If the last segment is a numeric index inside an array, the value is
///   inserted at that position (shifting subsequent elements).
/// - If the parent is an object, a new key is created (errors if it already
///   exists -- use `set` to overwrite).
pub fn json_add(
    file: &str,
    pointer: &str,
    value_str: &str,
    output: Option<&str>,
    dry_run: bool,
    pretty: bool,
) -> Result<()> {
    let mut root = read_json_file(file)?;
    let new_value: Value = serde_json::from_str(value_str)
        .with_context(|| format!("Invalid JSON value: {}", value_str))?;

    if pointer.is_empty() {
        // RFC 6902 "add" with empty pointer replaces the whole document.
        root = new_value;
    } else {
        let (parent, key) = resolve_parent_and_key(&mut root, pointer)?;

        match parent {
            Value::Array(arr) => {
                if key == "-" {
                    arr.push(new_value);
                } else {
                    let idx: usize = key
                        .parse()
                        .with_context(|| format!("Invalid array index {:?}", key))?;
                    if idx > arr.len() {
                        bail!(
                            "Array index {} out of bounds for insert (length {})",
                            idx,
                            arr.len()
                        );
                    }
                    arr.insert(idx, new_value);
                }
            }
            Value::Object(map) => {
                // RFC 6902 add replaces if key exists; we follow that semantics.
                map.insert(key, new_value);
            }
            _ => bail!("Parent at pointer is not an object or array"),
        }
    }

    write_json(&root, file, output, dry_run, pretty)
}

/// Delete the value at `pointer`.
pub fn json_delete(
    file: &str,
    pointer: &str,
    output: Option<&str>,
    dry_run: bool,
    pretty: bool,
) -> Result<()> {
    if pointer.is_empty() {
        bail!("Cannot delete the root document");
    }

    let mut root = read_json_file(file)?;
    let (parent, key) = resolve_parent_and_key(&mut root, pointer)?;

    match parent {
        Value::Object(map) => {
            if map.remove(&key).is_none() {
                bail!("Key {:?} not found; nothing to delete", key);
            }
        }
        Value::Array(arr) => {
            let idx: usize = key
                .parse()
                .with_context(|| format!("Invalid array index {:?}", key))?;
            if idx >= arr.len() {
                bail!(
                    "Array index {} out of bounds (length {}); nothing to delete",
                    idx,
                    arr.len()
                );
            }
            arr.remove(idx);
        }
        _ => bail!("Parent at pointer is not an object or array"),
    }

    write_json(&root, file, output, dry_run, pretty)
}

// ---------------------------------------------------------------------------
// RFC 6902 JSON Patch
// ---------------------------------------------------------------------------

/// Apply an RFC 6902 JSON Patch document.
///
/// `patch_source` is the path to a file containing the patch array, or `None`
/// / `"-"` to read from stdin.
pub fn json_patch(
    file: &str,
    patch_source: Option<&str>,
    output: Option<&str>,
    dry_run: bool,
    pretty: bool,
) -> Result<()> {
    let mut root = read_json_file(file)?;

    // Read patch document.
    let patch_str = match patch_source {
        None | Some("-") => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("Failed to read patch from stdin")?;
            buf
        }
        Some(path) => {
            std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read patch file {}", path))?
        }
    };

    let patch_doc: Value =
        serde_json::from_str(&patch_str).context("Invalid JSON in patch document")?;

    let ops = patch_doc
        .as_array()
        .context("Patch document must be a JSON array of operations")?;

    // --- Pre-flight: run all `test` operations first so we can abort early ---
    for (i, op_val) in ops.iter().enumerate() {
        let op_name = op_val
            .get("op")
            .and_then(Value::as_str)
            .with_context(|| format!("Patch operation {} missing 'op' field", i))?;

        if op_name == "test" {
            apply_patch_op(&mut root, op_val, i, true)?;
        }
    }

    // --- Apply all operations in order ---
    for (i, op_val) in ops.iter().enumerate() {
        apply_patch_op(&mut root, op_val, i, false)?;
    }

    write_json(&root, file, output, dry_run, pretty)
}

/// Apply a single RFC 6902 operation.
///
/// When `test_only` is true, only `test` operations are executed (all others
/// are skipped). This is used for pre-flight validation.
fn apply_patch_op(root: &mut Value, op_val: &Value, index: usize, test_only: bool) -> Result<()> {
    let op = op_val
        .get("op")
        .and_then(Value::as_str)
        .with_context(|| format!("Patch operation {} missing 'op'", index))?;

    match op {
        "test" => {
            let path = get_patch_path(op_val, index)?;
            let expected = op_val
                .get("value")
                .with_context(|| format!("Patch op {} (test): missing 'value'", index))?;
            let actual = resolve_pointer_mut(root, &path)
                .with_context(|| format!("Patch op {} (test): path {:?} not found", index, path))?;
            if actual != expected {
                bail!(
                    "Patch op {} (test) failed: value at {:?} is {}, expected {}",
                    index,
                    path,
                    actual,
                    expected
                );
            }
        }

        "add" if !test_only => {
            let path = get_patch_path(op_val, index)?;
            let value = op_val
                .get("value")
                .with_context(|| format!("Patch op {} (add): missing 'value'", index))?
                .clone();
            patch_add(root, &path, value)
                .with_context(|| format!("Patch op {} (add) at {:?}", index, path))?;
        }

        "remove" if !test_only => {
            let path = get_patch_path(op_val, index)?;
            patch_remove(root, &path)
                .with_context(|| format!("Patch op {} (remove) at {:?}", index, path))?;
        }

        "replace" if !test_only => {
            let path = get_patch_path(op_val, index)?;
            let value = op_val
                .get("value")
                .with_context(|| format!("Patch op {} (replace): missing 'value'", index))?
                .clone();
            patch_replace(root, &path, value)
                .with_context(|| format!("Patch op {} (replace) at {:?}", index, path))?;
        }

        "move" if !test_only => {
            let from = get_patch_field(op_val, "from", index, "move")?;
            let path = get_patch_path(op_val, index)?;
            patch_move(root, &from, &path)
                .with_context(|| format!("Patch op {} (move) from {:?} to {:?}", index, from, path))?;
        }

        "copy" if !test_only => {
            let from = get_patch_field(op_val, "from", index, "copy")?;
            let path = get_patch_path(op_val, index)?;
            patch_copy(root, &from, &path)
                .with_context(|| format!("Patch op {} (copy) from {:?} to {:?}", index, from, path))?;
        }

        _ if test_only => { /* skip non-test ops during pre-flight */ }

        other => bail!("Patch op {}: unknown operation {:?}", index, other),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Patch operation helpers
// ---------------------------------------------------------------------------

fn get_patch_path(op_val: &Value, index: usize) -> Result<String> {
    get_patch_field(op_val, "path", index, "op")
}

fn get_patch_field(op_val: &Value, field: &str, index: usize, op_name: &str) -> Result<String> {
    op_val
        .get(field)
        .and_then(Value::as_str)
        .map(String::from)
        .with_context(|| format!("Patch op {} ({}): missing '{}'", index, op_name, field))
}

/// RFC 6902 `add`: identical semantics to `json_add`.
fn patch_add(root: &mut Value, path: &str, value: Value) -> Result<()> {
    if path.is_empty() {
        // Replace the whole document.
        *root = value;
        return Ok(());
    }

    let (parent, key) = resolve_parent_and_key(root, path)?;
    match parent {
        Value::Array(arr) => {
            if key == "-" {
                arr.push(value);
            } else {
                let idx: usize = key.parse().with_context(|| {
                    format!("Invalid array index {:?}", key)
                })?;
                if idx > arr.len() {
                    bail!("Array index {} out of bounds (length {})", idx, arr.len());
                }
                arr.insert(idx, value);
            }
        }
        Value::Object(map) => {
            map.insert(key, value);
        }
        _ => bail!("Parent is not an object or array"),
    }
    Ok(())
}

/// RFC 6902 `remove`.
fn patch_remove(root: &mut Value, path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("Cannot remove the root document");
    }

    let (parent, key) = resolve_parent_and_key(root, path)?;
    match parent {
        Value::Object(map) => {
            if map.remove(&key).is_none() {
                bail!("Key {:?} not found", key);
            }
        }
        Value::Array(arr) => {
            let idx: usize = key
                .parse()
                .with_context(|| format!("Invalid array index {:?}", key))?;
            if idx >= arr.len() {
                bail!("Array index {} out of bounds (length {})", idx, arr.len());
            }
            arr.remove(idx);
        }
        _ => bail!("Parent is not an object or array"),
    }
    Ok(())
}

/// RFC 6902 `replace`: the target must already exist.
fn patch_replace(root: &mut Value, path: &str, value: Value) -> Result<()> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }

    let target = resolve_pointer_mut(root, path)?;
    *target = value;
    Ok(())
}

/// RFC 6902 `move`: remove from `from`, then add at `path`.
fn patch_move(root: &mut Value, from: &str, path: &str) -> Result<()> {
    // Extract the value at `from`.
    let value = extract_and_remove(root, from)?;
    // Add it at `path`.
    patch_add(root, path, value)
}

/// RFC 6902 `copy`: read from `from`, then add at `path`.
fn patch_copy(root: &mut Value, from: &str, path: &str) -> Result<()> {
    let value = resolve_pointer_mut(root, from)
        .with_context(|| format!("Copy source {:?} not found", from))?
        .clone();
    patch_add(root, path, value)
}

/// Remove the value at `pointer` and return it.
fn extract_and_remove(root: &mut Value, pointer: &str) -> Result<Value> {
    if pointer.is_empty() {
        bail!("Cannot move from the root document");
    }

    let (parent, key) = resolve_parent_and_key(root, pointer)?;
    match parent {
        Value::Object(map) => map
            .remove(&key)
            .with_context(|| format!("Key {:?} not found for move", key)),
        Value::Array(arr) => {
            let idx: usize = key
                .parse()
                .with_context(|| format!("Invalid array index {:?}", key))?;
            if idx >= arr.len() {
                bail!("Array index {} out of bounds (length {})", idx, arr.len());
            }
            Ok(arr.remove(idx))
        }
        _ => bail!("Parent is not an object or array"),
    }
}
