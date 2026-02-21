# jsonai

Agent-first JSON full-text search CLI. Search JSON files and get back only the matching objects — no context waste.

Built on [Tantivy](https://github.com/quickwit-oss/tantivy) (in-memory indexing, no server required).

## Install

Download from [GitHub Releases](https://github.com/worktoolai/jsonai/releases) (Linux, macOS, Windows):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/worktoolai/jsonai/releases/latest/download/jsonai-darwin-arm64 -o jsonai
chmod +x jsonai && mv jsonai /usr/local/bin/
```

Or build from source:

```bash
cargo install --path .
```

## Quick Start

```bash
# Search all values
jsonai search -q "john" --all users.json

# Search specific field
jsonai search -q "developer" -f role users.json

# Search entire directory
jsonai search -q "timeout" --all ./configs/

# Pipe from stdin
curl https://api.example.com/data | jsonai search -q "error" --all -

# Modify JSON
jsonai set -p /0/name '"New Name"' users.json
```

## Global Flags

| Flag | Description |
|---|---|
| `--pretty` | Pretty-print JSON output (stdout defaults to compact) |
| `--compact` | Compact JSON output (file writes default to pretty) |

Defaults are optimized for agents: stdout is compact to save tokens, file writes are pretty for human readability.

## Commands

### `search`

```
jsonai search [OPTIONS] -q <QUERY> <INPUT>
```

`<INPUT>` is a file path, directory, glob pattern, or `-` for stdin.

#### Search options

| Flag | Short | Description | Default |
|---|---|---|---|
| `--query` | `-q` | Search query string | required |
| `--field` | `-f` | Search in specific field (repeatable) | |
| `--all` | `-a` | Search across all values | default if no `-f` |
| `--match` | `-m` | Match mode: `text` `exact` `fuzzy` `regex` | `text` |

#### Output options

| Flag | Short | Description | Default |
|---|---|---|---|
| `--output` | `-o` | Output mode: `match` `hit` `value` | `match` |
| `--limit` | `-l` | Max results | `20` |
| `--offset` | | Skip first N results | `0` |
| `--count-only` | | Return count only, no results | |
| `--select` | | Project specific fields (comma-separated) | |
| `--bare` | | Output bare JSON array, no envelope | |
| `--max-bytes` | | Max output bytes (results truncated to fit, JSON stays valid) | |
| `--schema` | | JSON Schema file for structure awareness | |

#### Overflow protection

When a search returns too many results, jsonai returns a **plan** instead of dumping everything — helping agents narrow their search.

| Flag | Description | Default |
|---|---|---|
| `--threshold` | Result count that triggers plan mode | `50` |
| `--plan` | Force plan mode (always return plan, no results) | |
| `--no-overflow` | Bypass overflow protection, always return results | |

Plan mode output includes:

- **fields**: all field names with distinct value counts (sorted by cardinality)
- **facets**: value distributions for low-cardinality fields (top 5 values)
- **commands**: ready-to-run `jsonai` commands for narrowing by each facet field

```bash
# Triggers plan mode if >50 results
jsonai search -q "error" --all ./logs/

# Force plan mode to explore the data
jsonai search -q "error" --all --plan ./logs/

# Bypass overflow, get all results
jsonai search -q "error" --all --no-overflow ./logs/
```

### `fields`

List all searchable field paths in a JSON file.

```bash
jsonai fields data.json
```

```json
["email","id","name","role","tags"]
```

### `set`

Set/update a value at a JSON Pointer path.

```bash
jsonai set -p /0/name '"New Name"' users.json
jsonai set -p /database/port '5433' config.json
jsonai set -p /0/name '"Test"' users.json --dry-run    # preview without writing
jsonai set -p /0/name '"Test"' users.json -o out.json  # write to different file
```

### `add`

Add a value at a JSON Pointer path (append to arrays, insert at index, add to objects).

```bash
jsonai add -p /users/- '{"id":6,"name":"New User"}' data.json  # append to array
jsonai add -p /users/0 '{"id":0,"name":"First"}' data.json     # insert at index 0
jsonai add -p /settings/theme '"dark"' config.json              # add to object
```

### `delete`

Delete a value at a JSON Pointer path.

```bash
jsonai delete -p /0/email users.json       # delete a field
jsonai delete -p /users/2 data.json        # delete array element
```

### `patch`

Apply a JSON Patch (RFC 6902) document. Supports operations: `test`, `add`, `remove`, `replace`, `move`, `copy`.

```bash
# Patch from file
jsonai patch -p patch.json target.json

# Patch from stdin
echo '[{"op":"replace","path":"/0/name","value":"Updated"}]' | jsonai patch -p - target.json
```

All manipulation commands support `--dry-run` (preview to stdout) and `-o <file>` (write to different file).

## Output Format

### Default: envelope

```bash
jsonai search -q "john" --all users.json
```

```json
{"meta":{"total":1,"returned":1,"limit":20,"truncated":false,"files_searched":1},"results":[{"id":1,"name":"John Doe","email":"john@example.com","role":"admin"}]}
```

`meta.truncated` tells the agent if there are more results beyond the limit or byte budget.

### `--bare`

```json
[{"id":1,"name":"John Doe","email":"john@example.com","role":"admin"}]
```

### `--output hit`

Includes file path, JSON Pointer (RFC 6901), and relevance score.

```json
{"meta":{"total":1,"returned":1,"limit":20,"truncated":false},"hits":[{"file":"users.json","pointer":"/0","record":{"id":1,"name":"John Doe"},"score":1.906}]}
```

### `--output value`

Returns only the matched values.

### `--count-only`

```json
{"meta":{"total":5,"returned":0,"limit":20,"truncated":false}}
```

### `--max-bytes`

Truncate results to fit within a byte budget. JSON remains valid; `meta.truncated` indicates overflow.

```bash
jsonai search -q "error" --all --max-bytes 4096 logs.json
```

## Match Modes

```bash
# text (default) — tokenized full-text search
jsonai search -q "john doe" --all data.json

# exact — exact value match
jsonai search -q "admin" -f role -m exact data.json

# fuzzy — edit distance tolerance
jsonai search -q "jon" --all -m fuzzy data.json

# regex — regular expression
jsonai search -q "^j.*@example" --all -m regex data.json
```

## Multi-file Search

```bash
# Directory (recursive, all *.json files)
jsonai search -q "error" --all ./logs/

# Glob pattern
jsonai search -q "error" --all "./**/*.json"
```

Results from multiple files are merged. Use `--output hit` to see which file each result came from.

## Field Projection

```bash
jsonai search -q "example.com" --all --select "name,email" users.json
```

```json
{"meta":{"total":3,"returned":3,"limit":20,"truncated":false},"results":[{"name":"John Doe","email":"john@example.com"},{"name":"Jane Smith","email":"jane@example.com"},{"name":"Alice Kim","email":"alice@example.com"}]}
```

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Matches found / command succeeded |
| `1` | No matches (not an error) |
| `2` | Error (parse, runtime) |

Errors go to stderr. stdout is always clean JSON (or empty).

## How It Works

1. Reads JSON file(s) and walks the tree to extract every object at every nesting level
2. Indexes all objects in Tantivy (in-memory, no disk)
3. Searches using the specified query and match mode
4. Deduplicates: if both a parent and child object match, returns only the deepest (most specific) one
5. Formats and outputs the matching objects

## License

MIT
