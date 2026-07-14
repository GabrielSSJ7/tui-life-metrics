use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection, Row};

use crate::models::{AttrMap, Entry, ParsedAction, UNPROCESSED};

/// Owns the SQLite connection and all entry persistence.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite db at {}", path.display()))?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(SCHEMA)
            .context("running migrations")?;
        Ok(())
    }

    /// Insert a successfully parsed entry; returns its row id.
    pub fn insert(&self, raw: &str, parsed: &ParsedAction) -> Result<i64> {
        let attrs = serde_json::to_string(&parsed.attributes)?;
        self.conn.execute(
            "INSERT INTO entries (raw_text, category, occurred_on, attributes, note, processed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, datetime('now'))",
            params![raw, parsed.category, parsed.occurred_on.to_string(), attrs, opt(&parsed.note)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Store an unparsed sentence for later reprocessing (offline fallback).
    pub fn insert_unprocessed(&self, raw: &str, occurred_on: NaiveDate) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO entries (raw_text, category, occurred_on, attributes, note, processed, created_at)
             VALUES (?1, ?2, ?3, '{}', NULL, 0, datetime('now'))",
            params![raw, UNPROCESSED, occurred_on.to_string()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Overwrite a previously unprocessed entry with parsed data.
    pub fn update_processed(&self, id: i64, parsed: &ParsedAction) -> Result<()> {
        let attrs = serde_json::to_string(&parsed.attributes)?;
        self.conn.execute(
            "UPDATE entries SET category=?1, occurred_on=?2, attributes=?3, note=?4, processed=1
             WHERE id=?5",
            params![
                parsed.category,
                parsed.occurred_on.to_string(),
                attrs,
                opt(&parsed.note),
                id
            ],
        )?;
        Ok(())
    }

    /// Entries whose `occurred_on` falls in the inclusive `[start, end]` window.
    pub fn entries_between(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, raw_text, category, occurred_on, attributes, note, processed, created_at
             FROM entries WHERE occurred_on BETWEEN ?1 AND ?2
             ORDER BY occurred_on DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![start.to_string(), end.to_string()], row_to_entry)?;
        collect(rows)
    }

    /// Entries still awaiting a Claude parse.
    pub fn unprocessed(&self) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, raw_text, category, occurred_on, attributes, note, processed, created_at
             FROM entries WHERE processed=0 ORDER BY id",
        )?;
        let rows = stmt.query_map([], row_to_entry)?;
        collect(rows)
    }

    /// Permanently remove an entry by id. Returns the number of rows deleted.
    pub fn delete(&self, id: i64) -> Result<usize> {
        let removed = self
            .conn
            .execute("DELETE FROM entries WHERE id=?1", params![id])?;
        Ok(removed)
    }
}

fn opt(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn collect(rows: impl Iterator<Item = rusqlite::Result<Entry>>) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn row_to_entry(row: &Row) -> rusqlite::Result<Entry> {
    let occurred: String = row.get(3)?;
    let attrs_json: String = row.get(4)?;
    Ok(Entry {
        id: row.get(0)?,
        raw_text: row.get(1)?,
        category: row.get(2)?,
        occurred_on: NaiveDate::parse_from_str(&occurred, "%Y-%m-%d").unwrap_or_else(|_| epoch()),
        attributes: parse_attrs(&attrs_json),
        note: row.get(5)?,
        processed: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
    })
}

/// Attributes JSON is best-effort: a corrupt bag degrades to empty, never crashes.
fn parse_attrs(json: &str) -> AttrMap {
    serde_json::from_str(json).unwrap_or_default()
}

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch is valid")
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS entries (
    id          INTEGER PRIMARY KEY,
    raw_text    TEXT NOT NULL,
    category    TEXT NOT NULL,
    occurred_on TEXT NOT NULL,
    attributes  TEXT NOT NULL DEFAULT '{}',
    note        TEXT,
    processed   INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_entries_occurred ON entries(occurred_on);
CREATE INDEX IF NOT EXISTS idx_entries_category ON entries(category);
";
