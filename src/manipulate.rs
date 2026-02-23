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

/// Re-escape a segment for embedding back into a JSON Pointer string.
/// `~` -> `~0`, then `/` -> `~1`.
fn escape_segment(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

/// Validate that all `~` escapes in a pointer segment are RFC 6901-compliant.
fn validate_escaped_segment(seg: &str) -> Result<()> {
    let mut chars = seg.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.next() {
                Some('0') | Some('1') => {}
                Some(other) => {
                    bail!(
                        "Invalid JSON Pointer escape '~{}' in segment {:?}",
                        other,
                        seg
                    )
                }
                None => bail!(
                    "Invalid JSON Pointer escape '~' at end of segment {:?}",
                    seg
                ),
            }
        }
    }
    Ok(())
}

/// Build a JSON Pointer string from unescaped segments.
fn build_pointer_from_segments(segments: &[String]) -> String {
    if segments.is_empty() {
        String::new()
    } else {
        format!(
            "/{}",
            segments
                .iter()
                .map(|s| escape_segment(s))
                .collect::<Vec<String>>()
                .join("/")
        )
    }
}

/// Normalize pointer text for error messages:
/// - keep root pointer as ""
/// - when input is a valid pointer, return canonical escaped form
/// - for non-pointer slash paths, escape each slash-delimited segment
fn normalize_pointer_for_error(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    if path.starts_with('/') {
        if let Ok(segments) = parse_pointer_segments(path) {
            return build_pointer_from_segments(&segments);
        }
        return path.to_string();
    }

    build_pointer_from_segments(
        &path
            .split('/')
            .map(|segment| segment.to_string())
            .collect::<Vec<String>>(),
    )
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

    parse_pointer_segments(pointer)
}

fn parse_pointer_segments(pointer: &str) -> Result<Vec<String>> {
    pointer[1..]
        .split('/')
        .enumerate()
        .map(|(idx, seg)| {
            validate_escaped_segment(seg).with_context(|| {
                format!(
                    "Invalid escape in JSON Pointer {:?} at segment {}",
                    pointer, idx
                )
            })?;
            Ok(unescape_segment(seg))
        })
        .collect()
}

/// Navigate a JSON Pointer to obtain a mutable reference to the target value.
fn resolve_pointer_mut<'a>(root: &'a mut Value, pointer: &str) -> Result<&'a mut Value> {
    let segments = parse_pointer(pointer)?;
    let mut current = root;

    for (i, seg) in segments.iter().enumerate() {
        let built = build_pointer_from_segments(&segments[..=i]);

        current = match current {
            Value::Object(map) => map
                .get_mut(seg)
                .with_context(|| format!("Key {:?} not found at pointer {:?}", seg, built))?,
            Value::Array(arr) => {
                let idx: usize = seg
                    .parse()
                    .with_context(|| format!("Invalid array index {:?} at {:?}", seg, built))?;
                arr.get_mut(idx)
                    .with_context(|| format!("Array index {} out of bounds at {:?}", idx, built))?
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

    // Build parent pointer from all segments except the last, re-escaping
    // each segment so that keys containing `/` or `~` survive a round-trip
    // through `parse_pointer` → `resolve_pointer_mut`.
    let parent_path = build_pointer_from_segments(&segments[..segments.len() - 1]);

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
fn write_json(
    value: &Value,
    file: &str,
    output: Option<&str>,
    dry_run: bool,
    pretty: bool,
) -> Result<()> {
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
                    bail!("Array index {} out of bounds (length {})", idx, arr.len());
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
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read patch file {}", path))?,
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
            let display_path = normalize_pointer_for_error(&path);
            let expected = op_val
                .get("value")
                .with_context(|| format!("Patch op {} (test): missing 'value'", index))?;
            let actual = resolve_pointer_mut(root, &path).with_context(|| {
                format!(
                    "Patch op {} (test): path {:?} not found",
                    index, display_path
                )
            })?;
            if actual != expected {
                bail!(
                    "Patch op {} (test) failed: value at {:?} is {}, expected {}",
                    index,
                    display_path,
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
            patch_move(root, &from, &path).with_context(|| {
                format!("Patch op {} (move) from {:?} to {:?}", index, from, path)
            })?;
        }

        "copy" if !test_only => {
            let from = get_patch_field(op_val, "from", index, "copy")?;
            let path = get_patch_path(op_val, index)?;
            patch_copy(root, &from, &path).with_context(|| {
                format!("Patch op {} (copy) from {:?} to {:?}", index, from, path)
            })?;
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
                let idx: usize = key
                    .parse()
                    .with_context(|| format!("Invalid array index {:?}", key))?;
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
    // RFC 6902: moving a value into one of its descendants is invalid.
    if from == path || path.starts_with(&(from.to_string() + "/")) {
        bail!(
            "Invalid move: destination {:?} is the same as or inside source {:?}",
            normalize_pointer_for_error(path),
            normalize_pointer_for_error(from)
        );
    }

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_unescape_segment() {
        // Test ~1 (/) unescaping
        assert_eq!(unescape_segment("foo~1bar"), "foo/bar");
        assert_eq!(unescape_segment("src~1lib~1hooks"), "src/lib/hooks");

        // Test ~0 (~) unescaping
        assert_eq!(unescape_segment("config~0backup"), "config~backup");
        assert_eq!(unescape_segment("a~0b"), "a~b");

        // Test mixed ~1 and ~0
        assert_eq!(
            unescape_segment("path~1to~1file~0name"),
            "path/to/file~name"
        );

        // Test no escapes needed
        assert_eq!(unescape_segment("simple"), "simple");
        assert_eq!(unescape_segment(""), "");

        // Test order: ~1 is replaced first, then ~0
        // For ~01: no ~1 found, then ~0 -> ~ gives ~1
        assert_eq!(unescape_segment("~01"), "~1");

        // Test ~00 (tilde followed by zero)
        // ~00 -> ~0 (since ~0 -> ~)
        assert_eq!(unescape_segment("~00"), "~0");

        // Test ~10 (one followed by zero)
        // ~10 -> /0 (since ~1 -> /)
        assert_eq!(unescape_segment("~10"), "/0");

        // Test ~1~0 (slash then tilde)
        // ~1~0 -> /~ (since ~1 -> / first, then ~0 -> ~)
        assert_eq!(unescape_segment("~1~0"), "/~");
    }

    #[test]
    fn test_escape_segment() {
        // Test / escaping
        assert_eq!(escape_segment("foo/bar"), "foo~1bar");
        assert_eq!(escape_segment("src/lib/hooks"), "src~1lib~1hooks");

        // Test ~ escaping
        assert_eq!(escape_segment("config~backup"), "config~0backup");
        assert_eq!(escape_segment("a~b"), "a~0b");

        // Test mixed / and ~
        assert_eq!(escape_segment("path/to/file~name"), "path~1to~1file~0name");

        // Test no escapes needed
        assert_eq!(escape_segment("simple"), "simple");
        assert_eq!(escape_segment(""), "");

        // Test order: ~ is escaped first, then /
        // This ensures ~ in the original doesn't get affected by / escaping
    }

    #[test]
    fn test_escape_roundtrip() {
        // Test that escape -> unescape is identity
        let cases = vec![
            "simple",
            "foo/bar",
            "src/lib/hooks",
            "config~backup",
            "a~b",
            "path/to/file~name",
            "complex/path/with/both~and/slashes",
            "",
            "~0",
            "~1",
            "~01",
        ];

        for original in cases {
            let escaped = escape_segment(original);
            let unescaped = unescape_segment(&escaped);
            assert_eq!(
                unescaped, original,
                "Roundtrip failed: {} -> {} -> {}",
                original, escaped, unescaped
            );
        }
    }

    #[test]
    fn test_parse_pointer() {
        // Test root pointer
        assert_eq!(parse_pointer("").unwrap(), Vec::<String>::new());

        // Test simple pointer
        assert_eq!(parse_pointer("/foo").unwrap(), vec!["foo".to_string()]);
        assert_eq!(
            parse_pointer("/foo/bar").unwrap(),
            vec!["foo".to_string(), "bar".to_string()]
        );

        // Test escaped pointer
        assert_eq!(
            parse_pointer("/foo~1bar").unwrap(),
            vec!["foo/bar".to_string()]
        );
        assert_eq!(
            parse_pointer("/src~1lib~1hooks").unwrap(),
            vec!["src/lib/hooks".to_string()]
        );
        assert_eq!(
            parse_pointer("/config~0backup").unwrap(),
            vec!["config~backup".to_string()]
        );

        // Test mixed escapes
        assert_eq!(
            parse_pointer("/path~1to~1file~0name").unwrap(),
            vec!["path/to/file~name".to_string()]
        );

        // Test numeric segments (for arrays)
        assert_eq!(parse_pointer("/0").unwrap(), vec!["0".to_string()]);
        assert_eq!(
            parse_pointer("/foo/0/bar").unwrap(),
            vec!["foo".to_string(), "0".to_string(), "bar".to_string()]
        );

        // Test error: pointer must start with /
        assert!(parse_pointer("foo").is_err());
        assert!(parse_pointer("foo/bar").is_err());

        // Test error: invalid RFC 6901 escapes
        assert!(parse_pointer("/~").is_err());
        assert!(parse_pointer("/~2").is_err());
        assert!(parse_pointer("/foo~bar").is_err());
    }

    #[test]
    fn test_resolve_pointer_mut() {
        let mut json = json!({
            "foo": "bar",
            "nested": {
                "key": "value"
            },
            "array": [1, 2, 3],
            "src/lib": {
                "hooks": "test"
            },
            "config~backup": true
        });

        // Test simple navigation
        let result = resolve_pointer_mut(&mut json, "/foo").unwrap();
        assert_eq!(result, &json!("bar"));

        // Test nested navigation
        let result = resolve_pointer_mut(&mut json, "/nested/key").unwrap();
        assert_eq!(result, &json!("value"));

        // Test array navigation
        let result = resolve_pointer_mut(&mut json, "/array/0").unwrap();
        assert_eq!(result, &json!(1));
        let result = resolve_pointer_mut(&mut json, "/array/2").unwrap();
        assert_eq!(result, &json!(3));

        // Test navigation with escaped slash
        let result = resolve_pointer_mut(&mut json, "/src~1lib").unwrap();
        assert_eq!(result, &json!({"hooks": "test"}));

        // Test navigation with escaped tilde
        let result = resolve_pointer_mut(&mut json, "/config~0backup").unwrap();
        assert_eq!(result, &json!(true));

        // Test nested navigation with escapes
        let result = resolve_pointer_mut(&mut json, "/src~1lib/hooks").unwrap();
        assert_eq!(result, &json!("test"));

        // Test error: key not found
        assert!(resolve_pointer_mut(&mut json, "/nonexistent").is_err());
        assert!(resolve_pointer_mut(&mut json, "/nested/nonexistent").is_err());

        // Test error: array out of bounds
        assert!(resolve_pointer_mut(&mut json, "/array/10").is_err());

        // Test error: navigating into primitive
        assert!(resolve_pointer_mut(&mut json, "/foo/bar").is_err());
    }

    #[test]
    fn test_resolve_parent_and_key() {
        let mut json = json!({
            "foo": "bar",
            "nested": {
                "key": "value"
            },
            "array": [1, 2, 3],
            "src/lib": {
                "hooks": "test"
            },
            "config~backup": true
        });

        // Test simple pointer
        {
            let (parent, key) = resolve_parent_and_key(&mut json, "/foo").unwrap();
            // Check that parent is the root by verifying it contains "foo"
            if let Value::Object(map) = parent {
                assert!(map.contains_key("foo"));
            } else {
                panic!("Parent should be an object");
            }
            assert_eq!(key, "foo");
        }

        // Test nested pointer
        {
            let (parent, key) = resolve_parent_and_key(&mut json, "/nested/key").unwrap();
            if let Value::Object(map) = parent {
                assert_eq!(map.get("key"), Some(&json!("value")));
            } else {
                panic!("Parent should be an object");
            }
            assert_eq!(key, "key");
        }

        // Test pointer with escaped slash
        {
            let (parent, key) = resolve_parent_and_key(&mut json, "/src~1lib/hooks").unwrap();
            if let Value::Object(map) = parent {
                assert_eq!(map.get("hooks"), Some(&json!("test")));
            } else {
                panic!("Parent should be an object");
            }
            assert_eq!(key, "hooks");
        }

        // Test pointer with escaped tilde
        {
            let (parent, key) = resolve_parent_and_key(&mut json, "/config~0backup").unwrap();
            // Check that parent is the root by verifying it contains "config~backup"
            if let Value::Object(map) = parent {
                assert!(map.contains_key("config~backup"));
            } else {
                panic!("Parent should be an object");
            }
            assert_eq!(key, "config~backup");
        }

        // Test pointer with mixed escapes
        {
            let (parent, key) = resolve_parent_and_key(&mut json, "/src~1lib").unwrap();
            // Check that parent is the root by verifying it contains "src/lib"
            if let Value::Object(map) = parent {
                assert!(map.contains_key("src/lib"));
            } else {
                panic!("Parent should be an object");
            }
            assert_eq!(key, "src/lib");
        }

        // Test array pointer
        {
            let (parent, key) = resolve_parent_and_key(&mut json, "/array/1").unwrap();
            if let Value::Array(arr) = parent {
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[1], 2);
            } else {
                panic!("Parent should be an array");
            }
            assert_eq!(key, "1");
        }

        // Test error: root pointer
        assert!(resolve_parent_and_key(&mut json, "").is_err());
    }

    #[test]
    fn test_set_with_slash_in_key() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "{{\"src/lib\": {{\"hooks\": \"old\"}}}}").unwrap();

        json_set(
            temp_file.path().to_str().unwrap(),
            "/src~1lib/hooks",
            "\"new\"",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"src/lib": {"hooks": "new"}}));
    }

    #[test]
    fn test_set_with_tilde_in_key() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "{{\"config~backup\": \"old\"}}").unwrap();

        json_set(
            temp_file.path().to_str().unwrap(),
            "/config~0backup",
            "\"new\"",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"config~backup": "new"}));
    }

    #[test]
    fn test_set_with_mixed_escapes() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "{{\"path/to/file~name\": \"old\"}}").unwrap();

        json_set(
            temp_file.path().to_str().unwrap(),
            "/path~1to~1file~0name",
            "\"new\"",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"path/to/file~name": "new"}));
    }

    #[test]
    fn test_add_with_slash_in_key() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "{{\"src/lib\": {{}}}}").unwrap();

        json_add(
            temp_file.path().to_str().unwrap(),
            "/src~1lib/hooks",
            "\"test\"",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"src/lib": {"hooks": "test"}}));
    }

    #[test]
    fn test_add_with_tilde_in_key() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "{{}}").unwrap();

        json_add(
            temp_file.path().to_str().unwrap(),
            "/config~0backup",
            "true",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"config~backup": true}));
    }

    #[test]
    fn test_add_with_mixed_escapes() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "{{}}").unwrap();

        json_add(
            temp_file.path().to_str().unwrap(),
            "/path~1to~1file~0name",
            "\"value\"",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"path/to/file~name": "value"}));
    }

    #[test]
    fn test_delete_with_slash_in_key() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            "{{\"src/lib\": {{\"hooks\": \"test\"}}, \"other\": \"keep\"}}"
        )
        .unwrap();

        json_delete(
            temp_file.path().to_str().unwrap(),
            "/src~1lib/hooks",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"src/lib": {}, "other": "keep"}));
    }

    #[test]
    fn test_delete_with_tilde_in_key() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            "{{\"config~backup\": true, \"other\": \"keep\"}}"
        )
        .unwrap();

        json_delete(
            temp_file.path().to_str().unwrap(),
            "/config~0backup",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"other": "keep"}));
    }

    #[test]
    fn test_delete_with_mixed_escapes() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            "{{\"path/to/file~name\": \"value\", \"other\": \"keep\"}}"
        )
        .unwrap();

        json_delete(
            temp_file.path().to_str().unwrap(),
            "/path~1to~1file~0name",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"other": "keep"}));
    }

    #[test]
    fn test_test_op_error_path_uses_pointer_escaping() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut target = NamedTempFile::new().unwrap();
        writeln!(target, "{{\"src/lib\": {{\"hooks\": \"actual\"}}}}").unwrap();

        let mut patch = NamedTempFile::new().unwrap();
        writeln!(
            patch,
            "[{{\"op\":\"test\",\"path\":\"/src~1lib/hooks\",\"value\":\"expected\"}}]"
        )
        .unwrap();

        let err = json_patch(
            target.path().to_str().unwrap(),
            Some(patch.path().to_str().unwrap()),
            None,
            false,
            true,
        )
        .unwrap_err();

        let msg = format!("{:#}", err);
        assert!(msg.contains("/src~1lib/hooks"));
        assert!(!msg.contains("/src/lib/hooks"));
    }

    #[test]
    fn test_complex_nested_structure_with_escapes() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            "{{\"src/lib\": {{\"sub~dir/nested\": {{\"value\": 1}}}}}}"
        )
        .unwrap();

        // Set a nested value
        json_set(
            temp_file.path().to_str().unwrap(),
            "/src~1lib/sub~0dir~1nested/value",
            "2",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(result, json!({"src/lib": {"sub~dir/nested": {"value": 2}}}));

        // Add a new key
        json_add(
            temp_file.path().to_str().unwrap(),
            "/src~1lib/sub~0dir~1nested/new~1key",
            "\"added\"",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(
            result,
            json!({"src/lib": {"sub~dir/nested": {"value": 2, "new/key": "added"}}})
        );

        // Delete a key
        json_delete(
            temp_file.path().to_str().unwrap(),
            "/src~1lib/sub~0dir~1nested/value",
            None,
            false,
            true,
        )
        .unwrap();

        let result = read_json_file(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(
            result,
            json!({"src/lib": {"sub~dir/nested": {"new/key": "added"}}})
        );
    }
}
