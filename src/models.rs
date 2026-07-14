use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Category used when a sentence could not be parsed (offline fallback).
pub const UNPROCESSED: &str = "unprocessed";

/// Attribute bag extracted from a sentence, e.g. `duration_min: 30`.
pub type AttrMap = BTreeMap<String, AttrValue>;

/// A single measurable fact. Numbers are aggregated; text/bool are shown only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttrValue {
    Num(f64),
    Bool(bool),
    Text(String),
}

impl AttrValue {
    /// The numeric value if this attribute is a number (used for summing).
    pub fn as_num(&self) -> Option<f64> {
        match self {
            AttrValue::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// Human-readable rendering, trimming trailing `.0` on whole numbers.
    pub fn display(&self) -> String {
        match self {
            AttrValue::Num(n) => trim_num(*n),
            AttrValue::Bool(b) => b.to_string(),
            AttrValue::Text(s) => s.clone(),
        }
    }
}

fn trim_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Structured result of parsing one sentence via the Claude subprocess.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ParsedAction {
    pub category: String,
    pub occurred_on: NaiveDate,
    #[serde(default)]
    pub attributes: AttrMap,
    #[serde(default)]
    pub note: String,
}

impl ParsedAction {
    /// Lowercase + trim the category so "Exercício " and "exercício" group together.
    pub fn normalized(mut self) -> Self {
        self.category = self.category.trim().to_lowercase();
        self
    }
}

/// A persisted life-log entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: i64,
    pub raw_text: String,
    pub category: String,
    pub occurred_on: NaiveDate,
    pub attributes: AttrMap,
    pub note: Option<String>,
    pub processed: bool,
    pub created_at: String,
}
