mod claude;
mod prompt;

pub use claude::ClaudeParser;
pub use prompt::extract;

use anyhow::Result;

use crate::models::ParsedAction;

/// Turns a free-text life-log sentence into structured data.
///
/// Implementations must be `Send + Sync` so the capture UI can run them on a
/// background thread while animating a spinner.
///
/// # Example
/// ```no_run
/// use tui_life_metrics::parser::{ActionParser, ClaudeParser};
/// let parser = ClaudeParser::default();
/// let action = parser.parse("Corri 5km hoje").unwrap();
/// println!("{}", action.category);
/// ```
pub trait ActionParser: Send + Sync {
    fn parse(&self, sentence: &str) -> Result<ParsedAction>;
}
