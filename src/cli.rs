use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "jsonai", about = "Agent-first JSON full-text search CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search JSON files by value
    Search(SearchArgs),
    /// List searchable fields from a JSON file or schema
    Fields(FieldsArgs),
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
