use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "jsonai", about = "Agent-first JSON full-text search CLI")]
pub struct Cli {
    /// Pretty-print JSON output (default for stdout: compact, for file writes: pretty)
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Compact JSON output (override pretty default for file writes)
    #[arg(long, global = true, conflicts_with = "pretty")]
    pub compact: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search JSON files by value
    Search(SearchArgs),
    /// List searchable fields from a JSON file or schema
    Fields(FieldsArgs),
    /// Set/update a field value at a JSON Pointer path
    Set(SetArgs),
    /// Add a value at a JSON Pointer path (append to arrays)
    Add(AddArgs),
    /// Delete a value at a JSON Pointer path
    Delete(DeleteArgs),
    /// Apply a JSON Patch (RFC 6902) document
    Patch(PatchArgs),
}

#[derive(Parser)]
pub struct SearchArgs {
    /// Search query string
    #[arg(short, long)]
    pub query: String,

    /// Search in specific field(s). Repeatable.
    #[arg(short, long)]
    pub field: Vec<String>,

    /// Search across all values (default if no --field given)
    #[arg(short, long)]
    pub all: bool,

    /// Match mode
    #[arg(short, long, value_enum, default_value_t = MatchMode::Text)]
    pub r#match: MatchMode,

    /// Output mode
    #[arg(short, long, value_enum, default_value_t = OutputMode::Match)]
    pub output: OutputMode,

    /// Max results to return
    #[arg(short, long, default_value_t = 20)]
    pub limit: usize,

    /// Skip first N results
    #[arg(long, default_value_t = 0)]
    pub offset: usize,

    /// Only return count, no results
    #[arg(long)]
    pub count_only: bool,

    /// Project specific fields in output (comma-separated)
    #[arg(long)]
    pub select: Option<String>,

    /// Output bare JSON array instead of envelope
    #[arg(long)]
    pub bare: bool,

    /// Max output bytes (results truncated to fit, JSON stays valid)
    #[arg(long)]
    pub max_bytes: Option<usize>,

    /// Overflow threshold: if results exceed this, return plan instead of results
    #[arg(long, default_value_t = 50)]
    pub threshold: usize,

    /// Force plan mode: return only metadata/facets/suggestions, no results
    #[arg(long)]
    pub plan: bool,

    /// Bypass overflow protection: always return results even if over threshold
    #[arg(long)]
    pub no_overflow: bool,

    /// JSON Schema file for structure awareness
    #[arg(long)]
    pub schema: Option<String>,

    /// Input: file path, directory, glob, or "-" for stdin
    #[arg(required = true)]
    pub input: String,
}

#[derive(Parser)]
pub struct FieldsArgs {
    /// JSON file or schema file to inspect
    pub input: String,

    /// Use this file as JSON Schema
    #[arg(long)]
    pub schema: bool,
}

#[derive(Parser)]
pub struct SetArgs {
    /// JSON Pointer path (e.g., /users/0/name)
    #[arg(short, long)]
    pub pointer: String,

    /// Value to set (JSON string, number, object, etc.)
    pub value: String,

    /// Target JSON file
    pub file: String,

    /// Write to a different file instead of in-place
    #[arg(short, long)]
    pub output: Option<String>,

    /// Dry run: print result without writing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Parser)]
pub struct AddArgs {
    /// JSON Pointer path (for arrays: /arr/- appends, /arr/0 inserts at index)
    #[arg(short, long)]
    pub pointer: String,

    /// Value to add (JSON)
    pub value: String,

    /// Target JSON file
    pub file: String,

    #[arg(short, long)]
    pub output: Option<String>,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Parser)]
pub struct DeleteArgs {
    /// JSON Pointer path to delete
    #[arg(short, long)]
    pub pointer: String,

    /// Target JSON file
    pub file: String,

    #[arg(short, long)]
    pub output: Option<String>,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Parser)]
pub struct PatchArgs {
    /// JSON Patch document (RFC 6902), or "-" for stdin
    #[arg(short, long)]
    pub patch: Option<String>,

    /// Target JSON file
    pub file: String,

    #[arg(short, long)]
    pub output: Option<String>,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Clone, ValueEnum)]
pub enum MatchMode {
    /// Tokenized full-text search (default)
    Text,
    /// Exact value match
    Exact,
    /// Fuzzy match (edit distance)
    Fuzzy,
    /// Regular expression
    Regex,
}

#[derive(Clone, ValueEnum)]
pub enum OutputMode {
    /// Matched JSON objects (default)
    Match,
    /// Matched objects with file path and JSON pointer
    Hit,
    /// Just matched values
    Value,
}
