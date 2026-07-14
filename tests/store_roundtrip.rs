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

#[test]
fn get_fetches_by_id_including_processed_and_offline() {
    let path = temp_db("get");
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).unwrap();

    let today = NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
    let processed_id = store.insert("Corri 30min", &parsed("exercício", 14, 30.0)).unwrap();
    let offline_id = store.insert_unprocessed("Algo offline", today).unwrap();

    assert_eq!(store.get(processed_id).unwrap().unwrap().category, "exercício");
    let offline = store.get(offline_id).unwrap().unwrap();
    assert!(!offline.processed);
    assert_eq!(offline.raw_text, "Algo offline");
    assert!(store.get(9999).unwrap().is_none());

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn delete_removes_entry() {
    let path = temp_db("delete");
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).unwrap();

    let id = store
        .insert("Corri 30min", &parsed("exercício", 14, 30.0))
        .unwrap();
    store
        .insert("Li um capítulo", &parsed("leitura", 14, 20.0))
        .unwrap();

    assert_eq!(store.delete(id).unwrap(), 1);
    assert_eq!(store.delete(id).unwrap(), 0); // already gone

    let all = store
        .entries_between(
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
        )
        .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].category, "leitura");

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
