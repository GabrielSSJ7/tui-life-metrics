use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDate};

use crate::models::Entry;

/// Aggregated totals for one category over a set of entries.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryTotal {
    pub category: String,
    pub count: usize,
    /// Summed numeric attributes keyed by attribute name (e.g. `duration_min`).
    pub sums: BTreeMap<String, f64>,
}

/// Count + numeric-attribute sums per category, sorted by count descending.
pub fn totals_by_category(entries: &[Entry]) -> Vec<CategoryTotal> {
    let mut acc: BTreeMap<String, CategoryTotal> = BTreeMap::new();
    for e in entries {
        let total = acc
            .entry(e.category.clone())
            .or_insert_with(|| CategoryTotal {
                category: e.category.clone(),
                count: 0,
                sums: BTreeMap::new(),
            });
        total.count += 1;
        accumulate_sums(&mut total.sums, e);
    }
    let mut out: Vec<_> = acc.into_values().collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.category.cmp(&b.category))
    });
    out
}

fn accumulate_sums(sums: &mut BTreeMap<String, f64>, e: &Entry) {
    for (key, val) in &e.attributes {
        if let Some(n) = val.as_num() {
            *sums.entry(key.clone()).or_insert(0.0) += n;
        }
    }
}

/// Distinct days on which any entry occurred.
pub fn active_days(entries: &[Entry]) -> BTreeSet<NaiveDate> {
    entries.iter().map(|e| e.occurred_on).collect()
}

/// Consecutive-day streak ending at `today` (or yesterday, if today is empty).
pub fn current_streak(entries: &[Entry], today: NaiveDate) -> u32 {
    let days = active_days(entries);
    let mut cursor = if days.contains(&today) {
        today
    } else if days.contains(&(today - Duration::days(1))) {
        today - Duration::days(1)
    } else {
        return 0;
    };
    let mut streak = 0;
    while days.contains(&cursor) {
        streak += 1;
        cursor -= Duration::days(1);
    }
    streak
}

/// Per-category count delta between two entry slices (current minus previous).
pub fn count_delta(current: &[Entry], previous: &[Entry]) -> BTreeMap<String, i64> {
    let cur = counts(current);
    let prev = counts(previous);
    let mut keys: BTreeSet<&String> = cur.keys().collect();
    keys.extend(prev.keys());
    keys.into_iter()
        .map(|k| {
            let delta = *cur.get(k).unwrap_or(&0) as i64 - *prev.get(k).unwrap_or(&0) as i64;
            (k.clone(), delta)
        })
        .collect()
}

fn counts(entries: &[Entry]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for e in entries {
        *map.entry(e.category.clone()).or_insert(0) += 1;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AttrValue;

    fn entry(cat: &str, day: u32, dur: Option<f64>) -> Entry {
        let mut attributes = BTreeMap::new();
        if let Some(d) = dur {
            attributes.insert("duration_min".to_string(), AttrValue::Num(d));
        }
        Entry {
            id: 0,
            raw_text: String::new(),
            category: cat.to_string(),
            occurred_on: NaiveDate::from_ymd_opt(2026, 7, day).unwrap(),
            attributes,
            note: None,
            processed: true,
            created_at: String::new(),
        }
    }

    #[test]
    fn totals_sum_numeric_attributes_and_sort_by_count() {
        let entries = vec![
            entry("exercício", 1, Some(30.0)),
            entry("exercício", 2, Some(45.0)),
            entry("leitura", 2, None),
        ];
        let totals = totals_by_category(&entries);
        assert_eq!(totals[0].category, "exercício");
        assert_eq!(totals[0].count, 2);
        assert_eq!(totals[0].sums.get("duration_min"), Some(&75.0));
        assert_eq!(totals[1].category, "leitura");
    }

    #[test]
    fn streak_counts_back_from_today() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
        let entries = vec![
            entry("x", 14, None),
            entry("x", 13, None),
            entry("x", 11, None),
        ];
        assert_eq!(current_streak(&entries, today), 2);
    }

    #[test]
    fn streak_zero_when_gap_before_today() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
        let entries = vec![entry("x", 12, None)];
        assert_eq!(current_streak(&entries, today), 0);
    }

    #[test]
    fn delta_reports_new_and_missing_categories() {
        let cur = vec![
            entry("a", 1, None),
            entry("a", 2, None),
            entry("b", 1, None),
        ];
        let prev = vec![entry("a", 1, None), entry("c", 1, None)];
        let delta = count_delta(&cur, &prev);
        assert_eq!(delta.get("a"), Some(&1));
        assert_eq!(delta.get("b"), Some(&1));
        assert_eq!(delta.get("c"), Some(&-1));
    }
}
