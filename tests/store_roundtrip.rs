//! End-to-end persistence checks against a real (temp) SQLite file.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use tui_life_metrics::models::{AttrValue, ParsedAction};
use tui_life_metrics::store::Store;

/// Unique temp DB path per test (no external tempfile crate needed).
fn temp_db(tag: &str) -> std::path::PathBuf {
    let name = format!("tlm-test-{}-{}.db", std::process::id(), tag);
    std::env::temp_dir().join(name)
}

fn parsed(category: &str, day: u32, dur: f64) -> ParsedAction {
    let mut attributes = BTreeMap::new();
    attributes.insert("duration_min".to_string(), AttrValue::Num(dur));
    ParsedAction {
        category: category.to_string(),
        occurred_on: NaiveDate::from_ymd_opt(2026, 7, day).unwrap(),
        attributes,
        note: "note".to_string(),
    }
}

#[test]
fn insert_then_query_within_window() {
    let path = temp_db("roundtrip");
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).unwrap();

    store
        .insert("Corri 30min", &parsed("exercício", 14, 30.0))
        .unwrap();
    store
        .insert("Li um capítulo", &parsed("leitura", 3, 20.0))
        .unwrap();

    let july = store
        .entries_between(
            NaiveDate::from_ymd_opt(2026, 7, 10).unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 20).unwrap(),
        )
        .unwrap();

    assert_eq!(july.len(), 1);
    assert_eq!(july[0].category, "exercício");
    assert_eq!(
        july[0].attributes.get("duration_min"),
        Some(&AttrValue::Num(30.0))
    );
    assert!(july[0].processed);

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn unprocessed_then_update_marks_processed() {
    let path = temp_db("reprocess");
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).unwrap();

    let today = NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
    let id = store.insert_unprocessed("Fiz algo offline", today).unwrap();
    assert_eq!(store.unprocessed().len_checked(), 1);

    store
        .update_processed(id, &parsed("exercício", 14, 15.0))
        .unwrap();
    assert_eq!(store.unprocessed().len_checked(), 0);

    std::fs::remove_file(&path).unwrap();
}

/// Small helper so the assertions above read cleanly.
trait LenChecked {
    fn len_checked(self) -> usize;
}
impl<T> LenChecked for anyhow::Result<Vec<T>> {
    fn len_checked(self) -> usize {
        self.unwrap().len()
    }
}
