# jsonai

Agent-first JSON full-text search CLI. Search JSON files and get back only the matching objects — no context waste.

Built on [Tantivy](https://github.com/quickwit-oss/tantivy) (in-memory indexing, no server required).

## Install

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
```

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
| `--schema` | | JSON Schema file for structure awareness | |

### `fields`

List all searchable field paths in a JSON file.

```bash
jsonai fields data.json
```

```json
["email", "id", "name", "role", "tags"]
```

For nested JSON:

```json
["app", "app.author", "app.name", "database", "database.host", "database.port"]
```

## Output Format

### Default: envelope

```bash
jsonai search -q "john" --all users.json
```

```json
{
  "meta": {
    "total": 2,
    "returned": 2,
    "limit": 20,
    "truncated": false,
    "files_searched": 1
  },
  "results": [
    {"id": 1, "name": "John Doe", "email": "john@example.com", "role": "admin"}
  ]
}
```

`meta.truncated` tells the agent if there are more results beyond the limit.

### `--bare`

```json
[
  {"id": 1, "name": "John Doe", "email": "john@example.com", "role": "admin"}
]
```

### `--output hit`

Includes file path, JSON Pointer (RFC 6901), and relevance score.

```json
{
  "meta": {"total": 1, "returned": 1, "limit": 20, "truncated": false},
  "hits": [
    {
      "file": "/path/to/users.json",
      "pointer": "/0",
      "record": {"id": 1, "name": "John Doe"},
      "score": 1.906
    }
  ]
}
```

### `--output value`

Returns only the matched values.

### `--count-only`

```json
{
  "meta": {"total": 5, "returned": 0, "limit": 20, "truncated": false}
}
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
{
  "meta": {"total": 3, "returned": 3, "limit": 20, "truncated": false},
  "results": [
    {"name": "John Doe", "email": "john@example.com"},
    {"name": "Jane Smith", "email": "jane@example.com"},
    {"name": "Alice Kim", "email": "alice@example.com"}
  ]
}
```

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | Matches found |
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
