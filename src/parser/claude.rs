use std::process::Command;

use anyhow::{anyhow, Result};
use chrono::Local;

use super::{prompt, ActionParser};
use crate::models::ParsedAction;

/// Parses sentences by shelling out to the `claude` CLI in print mode (`-p`).
///
/// Uses the user's existing Claude Code auth, so no API key handling is needed.
pub struct ClaudeParser {
    binary: String,
}

impl Default for ClaudeParser {
    fn default() -> Self {
        Self {
            binary: "claude".to_string(),
        }
    }
}

impl ClaudeParser {
    /// Override the binary name/path (mainly for tests or non-standard installs).
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl ActionParser for ClaudeParser {
    fn parse(&self, sentence: &str) -> Result<ParsedAction> {
        let today = Local::now().date_naive();
        let prompt = prompt::build(today, sentence);
        let output = Command::new(&self.binary)
            .arg("-p")
            .arg(&prompt)
            .output()
            .map_err(|e| anyhow!("running `{}` failed: {e}", self.binary))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("claude exited {}: {stderr}", output.status));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(prompt::extract(&stdout)?.normalized())
    }
}
