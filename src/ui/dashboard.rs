use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Duration, Local, NaiveDate};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};

use crate::metrics::{self, CategoryTotal};
use crate::models::Entry;
use crate::period::{Granularity, Period};
use crate::store::Store;

/// Window used to compute day-streaks regardless of the viewed period.
const STREAK_LOOKBACK_DAYS: i64 = 400;

/// Open the metrics dashboard, defaulting to the current month.
pub fn run(store: Store) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = Dashboard::new(store)?.run(&mut terminal);
    ratatui::restore();
    result
}

enum Mode {
    Summary,
    Drill,
}

struct Dashboard {
    store: Store,
    today: NaiveDate,
    period: Period,
    current: Vec<Entry>,
    totals: Vec<CategoryTotal>,
    deltas: BTreeMap<String, i64>,
    streaks: BTreeMap<String, u32>,
    table: TableState,
    mode: Mode,
    quit: bool,
}

impl Dashboard {
    fn new(store: Store) -> Result<Self> {
        let today = Local::now().date_naive();
        let mut dash = Self {
            store,
            today,
            period: Period::new(Granularity::Month, today),
            current: Vec::new(),
            totals: Vec::new(),
            deltas: BTreeMap::new(),
            streaks: BTreeMap::new(),
            table: TableState::default().with_selected(0),
            mode: Mode::Summary,
            quit: false,
        };
        dash.reload()?;
        Ok(dash)
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.on_key(key.code)?;
                }
            }
        }
        Ok(())
    }

    /// Recompute every aggregate for the current period from the store.
    fn reload(&mut self) -> Result<()> {
        let (start, end) = self.period.range();
        self.current = self.store.entries_between(start, end)?;
        let (pstart, pend) = self.period.previous().range();
        let previous = self.store.entries_between(pstart, pend)?;
        self.totals = metrics::totals_by_category(&self.current);
        self.deltas = metrics::count_delta(&self.current, &previous);
        self.streaks = self.compute_streaks()?;
        self.clamp_selection();
        Ok(())
    }

    fn compute_streaks(&self) -> Result<BTreeMap<String, u32>> {
        let recent = self.store.entries_between(
            self.today - Duration::days(STREAK_LOOKBACK_DAYS),
            self.today,
        )?;
        let mut by_cat: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
        for e in recent {
            by_cat.entry(e.category.clone()).or_default().push(e);
        }
        Ok(by_cat
            .into_iter()
            .map(|(cat, entries)| (cat, metrics::current_streak(&entries, self.today)))
            .collect())
    }

    fn clamp_selection(&mut self) {
        if self.totals.is_empty() {
            self.table.select(None);
        } else {
            let idx = self
                .table
                .selected()
                .unwrap_or(0)
                .min(self.totals.len() - 1);
            self.table.select(Some(idx));
        }
    }

    fn on_key(&mut self, code: KeyCode) -> Result<()> {
        match self.mode {
            Mode::Drill => self.on_drill_key(code),
            Mode::Summary => self.on_summary_key(code),
        }
    }

    fn on_summary_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('d') => self.set_gran(Granularity::Day)?,
            KeyCode::Char('w') => self.set_gran(Granularity::Week)?,
            KeyCode::Char('m') => self.set_gran(Granularity::Month)?,
            KeyCode::Char('y') => self.set_gran(Granularity::Year)?,
            KeyCode::Left | KeyCode::Char('h') => self.shift(-1)?,
            KeyCode::Right | KeyCode::Char('l') => self.shift(1)?,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Enter if !self.totals.is_empty() => self.mode = Mode::Drill,
            _ => {}
        }
        Ok(())
    }

    fn on_drill_key(&mut self, code: KeyCode) -> Result<()> {
        if matches!(code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace) {
            self.mode = Mode::Summary;
        }
        Ok(())
    }

    fn set_gran(&mut self, gran: Granularity) -> Result<()> {
        self.period = self.period.with_gran(gran);
        self.reload()
    }

    fn shift(&mut self, steps: i64) -> Result<()> {
        self.period = self.period.shift(steps);
        self.reload()
    }

    fn move_selection(&mut self, delta: isize) {
        if self.totals.is_empty() {
            return;
        }
        let last = self.totals.len() - 1;
        let cur = self.table.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, last as isize) as usize;
        self.table.select(Some(next));
    }

    fn selected_category(&self) -> Option<&str> {
        self.table
            .selected()
            .and_then(|i| self.totals.get(i))
            .map(|t| t.category.as_str())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let areas =
            Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(frame.area());
        frame.render_widget(self.header(), areas[0]);
        match self.mode {
            Mode::Summary => self.draw_summary(frame, areas[1]),
            Mode::Drill => self.draw_drill(frame, areas[1]),
        }
    }

    fn header(&self) -> Paragraph<'_> {
        let title = format!(
            " Métricas · {} ({}) ",
            self.period.label(),
            self.period.gran.label()
        );
        let hints = "d/w/m/y período   ←/→ navegar   ↑/↓ selecionar   Enter detalhes   q sair";
        Paragraph::new(Line::from(hints))
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(title))
    }

    fn draw_summary(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        if self.totals.is_empty() {
            let empty = Paragraph::new("sem registros neste período")
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        }
        let rows: Vec<Row> = self.totals.iter().map(|t| self.summary_row(t)).collect();
        let widths = [
            Constraint::Percentage(30),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Percentage(40),
        ];
        let table = Table::new(rows, widths)
            .header(header_row())
            .block(Block::default().borders(Borders::ALL))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(table, area, &mut self.table);
    }

    fn summary_row(&self, total: &CategoryTotal) -> Row<'static> {
        let delta = *self.deltas.get(&total.category).unwrap_or(&0);
        let streak = *self.streaks.get(&total.category).unwrap_or(&0);
        Row::new(vec![
            Cell::from(total.category.clone()),
            Cell::from(total.count.to_string()),
            Cell::from(delta_cell(delta)),
            Cell::from(format!("{streak}d")),
            Cell::from(sums_label(total)),
        ])
    }

    fn draw_drill(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let Some(category) = self.selected_category() else {
            return;
        };
        let items: Vec<ListItem> = self
            .current
            .iter()
            .filter(|e| e.category == category)
            .map(drill_item)
            .collect();
        let title = format!(" {category} · {}   [Esc volta] ", self.period.label());
        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
        frame.render_widget(list, area);
    }
}

fn header_row() -> Row<'static> {
    Row::new(["Categoria", "Qtd", "Δ", "Streak", "Métricas"])
        .style(Style::default().add_modifier(Modifier::BOLD))
}

fn delta_cell(delta: i64) -> Line<'static> {
    let (text, color) = match delta.cmp(&0) {
        std::cmp::Ordering::Greater => (format!("+{delta}"), Color::Green),
        std::cmp::Ordering::Less => (delta.to_string(), Color::Red),
        std::cmp::Ordering::Equal => ("0".to_string(), Color::DarkGray),
    };
    Line::styled(text, Style::default().fg(color))
}

fn sums_label(total: &CategoryTotal) -> String {
    total
        .sums
        .iter()
        .map(|(k, v)| format!("{k} {}", trim(*v)))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn drill_item(e: &Entry) -> ListItem<'static> {
    let attrs: Vec<String> = e
        .attributes
        .iter()
        .map(|(k, v)| format!("{k}={}", v.display()))
        .collect();
    let suffix = if attrs.is_empty() {
        String::new()
    } else {
        format!("  [{}]", attrs.join(", "))
    };
    ListItem::new(format!(
        "{}  {}{}",
        e.occurred_on.format("%d/%m"),
        e.raw_text,
        suffix
    ))
}

fn trim(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
