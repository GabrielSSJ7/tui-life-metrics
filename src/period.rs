use chrono::{Datelike, Duration, NaiveDate};

/// The time bucket used to group metrics in the dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Day,
    Week,
    Month,
    Year,
}

impl Granularity {
    pub fn label(self) -> &'static str {
        match self {
            Granularity::Day => "dia",
            Granularity::Week => "semana",
            Granularity::Month => "mês",
            Granularity::Year => "ano",
        }
    }
}

/// An inclusive date window plus the granularity that produced it.
#[derive(Debug, Clone, Copy)]
pub struct Period {
    pub gran: Granularity,
    pub anchor: NaiveDate,
}

impl Period {
    pub fn new(gran: Granularity, anchor: NaiveDate) -> Self {
        Self { gran, anchor }
    }

    /// Inclusive `(start, end)` covering the anchor's bucket.
    pub fn range(&self) -> (NaiveDate, NaiveDate) {
        match self.gran {
            Granularity::Day => (self.anchor, self.anchor),
            Granularity::Week => week_range(self.anchor),
            Granularity::Month => month_range(self.anchor),
            Granularity::Year => year_range(self.anchor),
        }
    }

    /// Move `steps` buckets forward (negative = backward).
    pub fn shift(&self, steps: i64) -> Self {
        let anchor = match self.gran {
            Granularity::Day => self.anchor + Duration::days(steps),
            Granularity::Week => self.anchor + Duration::weeks(steps),
            Granularity::Month => add_months(self.anchor, steps),
            Granularity::Year => add_months(self.anchor, steps * 12),
        };
        Self { anchor, ..*self }
    }

    /// The bucket immediately before this one, for trend deltas.
    pub fn previous(&self) -> Self {
        self.shift(-1)
    }

    pub fn with_gran(&self, gran: Granularity) -> Self {
        Self { gran, ..*self }
    }

    pub fn label(&self) -> String {
        let (start, end) = self.range();
        match self.gran {
            Granularity::Day => start.format("%d %b %Y").to_string(),
            Granularity::Week => format!("{} – {}", start.format("%d %b"), end.format("%d %b %Y")),
            Granularity::Month => start.format("%b %Y").to_string(),
            Granularity::Year => start.format("%Y").to_string(),
        }
    }
}

fn week_range(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    let offset = d.weekday().num_days_from_monday() as i64;
    let start = d - Duration::days(offset);
    (start, start + Duration::days(6))
}

fn month_range(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    let start = d.with_day(1).expect("day 1 is always valid");
    let end = add_months(start, 1) - Duration::days(1);
    (start, end)
}

fn year_range(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    let start = NaiveDate::from_ymd_opt(d.year(), 1, 1).expect("Jan 1 valid");
    let end = NaiveDate::from_ymd_opt(d.year(), 12, 31).expect("Dec 31 valid");
    (start, end)
}

/// Add `months` (may be negative), clamping the day to the target month length.
fn add_months(d: NaiveDate, months: i64) -> NaiveDate {
    let total = (d.year() as i64) * 12 + (d.month0() as i64) + months;
    let year = total.div_euclid(12) as i32;
    let month = total.rem_euclid(12) as u32 + 1;
    let day = d.day().min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).expect("clamped date is valid")
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(ny, nm, 1).expect("first of next month valid");
    (first_next - Duration::days(1)).day()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn month_range_covers_full_month() {
        let p = Period::new(Granularity::Month, date(2026, 2, 14));
        assert_eq!(p.range(), (date(2026, 2, 1), date(2026, 2, 28)));
    }

    #[test]
    fn add_months_clamps_day() {
        // Jan 31 + 1 month must clamp to Feb 28 (2026 is not a leap year).
        assert_eq!(add_months(date(2026, 1, 31), 1), date(2026, 2, 28));
    }

    #[test]
    fn previous_month_wraps_year() {
        let p = Period::new(Granularity::Month, date(2026, 1, 10));
        assert_eq!(p.previous().range().0, date(2025, 12, 1));
    }

    #[test]
    fn week_range_starts_monday() {
        // 2026-07-14 is a Tuesday.
        let p = Period::new(Granularity::Week, date(2026, 7, 14));
        assert_eq!(p.range(), (date(2026, 7, 13), date(2026, 7, 19)));
    }
}
