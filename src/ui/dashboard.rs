use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Local, NaiveDate};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState,
};
use ratatui::{DefaultTerminal, Frame};

use crate::metrics::{self, CategoryTotal};
use crate::models::Entry;
use crate::parser::ActionParser;
use crate::period::{Granularity, Period};
use crate::store::Store;

/// Window used to compute day-streaks regardless of the viewed period.
const STREAK_LOOKBACK_DAYS: i64 = 400;
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TICK: Duration = Duration::from_millis(90);

/// Open the metrics dashboard, defaulting to the current month.
///
/// `db_path` is where a background reprocess thread reopens the DB (the main
/// [`Store`] connection stays on this thread), and `parser` reparses offline
/// entries when the user presses `r`.
pub fn run(store: Store, parser: Arc<dyn ActionParser>, db_path: PathBuf) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = Dashboard::new(store, parser, db_path)?.run(&mut terminal);
    ratatui::restore();
    result
}

enum Mode {
    Summary,
    Drill,
}

struct Dashboard {
    store: Store,
    parser: Arc<dyn ActionParser>,
    db_path: PathBuf,
    today: NaiveDate,
    period: Period,
    current: Vec<Entry>,
    totals: Vec<CategoryTotal>,
    deltas: BTreeMap<String, i64>,
    streaks: BTreeMap<String, u32>,
    unprocessed_count: usize,
    table: TableState,
    mode: Mode,
    drill: ListState,
    /// Ids awaiting delete confirmation (one entry, or a whole category), if any.
    pending_delete: Option<Vec<i64>>,
    /// Channel receiving the reprocess result while a background parse runs.
    reprocess: Option<Receiver<Result<usize>>>,
    spinner: usize,
    quit: bool,
}

impl Dashboard {
    fn new(store: Store, parser: Arc<dyn ActionParser>, db_path: PathBuf) -> Result<Self> {
        let today = Local::now().date_naive();
        let mut dash = Self {
            store,
            parser,
            db_path,
            today,
            period: Period::new(Granularity::Month, today),
            current: Vec::new(),
            totals: Vec::new(),
            deltas: BTreeMap::new(),
            streaks: BTreeMap::new(),
            unprocessed_count: 0,
            table: TableState::default().with_selected(0),
            mode: Mode::Summary,
            drill: ListState::default().with_selected(Some(0)),
            pending_delete: None,
            reprocess: None,
            spinner: 0,
            quit: false,
        };
        dash.reload()?;
        Ok(dash)
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if self.reprocess.is_some() {
                self.tick_reprocess()?;
            } else if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.on_key(key.code)?;
                }
            }
        }
        Ok(())
    }

    /// Non-blocking cycle used while a background reprocess is running so the
    /// spinner animates and input still responds.
    fn tick_reprocess(&mut self) -> Result<()> {
        if event::poll(TICK)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.on_key(key.code)?;
                }
            }
        }
        self.spinner = (self.spinner + 1) % SPINNER.len();
        if let Some(rx) = &self.reprocess {
            if let Ok(result) = rx.try_recv() {
                result?;
                self.reprocess = None;
                self.reload()?;
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
        self.unprocessed_count = self.current.iter().filter(|e| !e.processed).count();
        self.clamp_selection();
        Ok(())
    }

    fn compute_streaks(&self) -> Result<BTreeMap<String, u32>> {
        let recent = self.store.entries_between(
            self.today - ChronoDuration::days(STREAK_LOOKBACK_DAYS),
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
        if self.pending_delete.is_some() {
            return self.on_confirm_key(code);
        }
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
            KeyCode::Char('r') => self.start_reprocess(),
            KeyCode::Char('X') => self.request_delete_category(),
            KeyCode::Left | KeyCode::Char('h') => self.shift(-1)?,
            KeyCode::Right | KeyCode::Char('l') => self.shift(1)?,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Enter if !self.totals.is_empty() => self.enter_drill(),
            _ => {}
        }
        Ok(())
    }

    fn on_drill_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => self.mode = Mode::Summary,
            KeyCode::Down | KeyCode::Char('j') => self.move_drill(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_drill(-1),
            KeyCode::Char('x') | KeyCode::Delete => {
                self.pending_delete = self.selected_drill_id().map(|id| vec![id]);
            }
            KeyCode::Char('r') => {
                if let Some(id) = self.selected_drill_id() {
                    self.start_reprocess_one(id);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Queue every entry of the selected category (in the current period) for
    /// deletion, pending y/n confirmation.
    fn request_delete_category(&mut self) {
        let ids: Vec<i64> = self.drill_entries().iter().map(|e| e.id).collect();
        if !ids.is_empty() {
            self.pending_delete = Some(ids);
        }
    }

    fn on_confirm_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Char('y') => self.confirm_delete()?,
            KeyCode::Char('n') | KeyCode::Esc => self.pending_delete = None,
            _ => {}
        }
        Ok(())
    }

    /// Delete every pending entry, reload, and leave the drill if it emptied out.
    fn confirm_delete(&mut self) -> Result<()> {
        let Some(ids) = self.pending_delete.take() else {
            return Ok(());
        };
        for id in ids {
            self.store.delete(id)?;
        }
        self.reload()?;
        let remaining = self.drill_entries().len();
        if remaining == 0 {
            self.mode = Mode::Summary;
        } else {
            let idx = self.drill.selected().unwrap_or(0).min(remaining - 1);
            self.drill.select(Some(idx));
        }
        Ok(())
    }

    /// Reprocess all offline entries (summary `r`).
    fn start_reprocess(&mut self) {
        if self.unprocessed_count == 0 {
            return;
        }
        self.spawn_reprocess(None);
    }

    /// Reprocess a single entry by id (drill `r`), reparsing its raw text —
    /// works on offline items and re-runs Claude on already-parsed ones to fix
    /// a wrong category.
    fn start_reprocess_one(&mut self, id: i64) {
        self.spawn_reprocess(Some(vec![id]));
    }

    /// Spawn a background thread that reparses entries via Claude. `ids = None`
    /// means every offline entry; `Some(ids)` targets those specific rows.
    ///
    /// The thread opens its own DB connection (SQLite handles the concurrent
    /// writes) so the UI thread's [`Store`] and event loop stay responsive.
    fn spawn_reprocess(&mut self, ids: Option<Vec<i64>>) {
        if self.reprocess.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let path = self.db_path.clone();
        let parser = Arc::clone(&self.parser);
        thread::spawn(move || {
            let _ = tx.send(reprocess(&path, parser.as_ref(), ids));
        });
        self.reprocess = Some(rx);
        self.spinner = 0;
    }

    fn enter_drill(&mut self) {
        self.mode = Mode::Drill;
        self.drill.select(Some(0));
        self.pending_delete = None;
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
        let cur = self.table.selected().unwrap_or(0);
        if let Some(next) = move_within(cur, self.totals.len(), delta) {
            self.table.select(Some(next));
        }
    }

    fn move_drill(&mut self, delta: isize) {
        let cur = self.drill.selected().unwrap_or(0);
        if let Some(next) = move_within(cur, self.drill_entries().len(), delta) {
            self.drill.select(Some(next));
        }
    }

    fn selected_category(&self) -> Option<&str> {
        self.table
            .selected()
            .and_then(|i| self.totals.get(i))
            .map(|t| t.category.as_str())
    }

    /// Entries in the current period belonging to the selected category.
    fn drill_entries(&self) -> Vec<&Entry> {
        match self.selected_category() {
            Some(cat) => self.current.iter().filter(|e| e.category == cat).collect(),
            None => Vec::new(),
        }
    }

    fn selected_drill_id(&self) -> Option<i64> {
        let entries = self.drill_entries();
        self.drill
            .selected()
            .and_then(|i| entries.get(i))
            .map(|e| e.id)
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
        Paragraph::new(Line::from(self.hint_text()))
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(title))
    }

    fn hint_text(&self) -> String {
        if let Some(ids) = &self.pending_delete {
            let noun = if ids.len() == 1 {
                "entrada"
            } else {
                "entradas"
            };
            return format!("deletar {} {noun}? (y confirma / n cancela)", ids.len());
        }
        if self.reprocess.is_some() {
            return format!(
                "{} reprocessando entradas offline com Claude…",
                SPINNER[self.spinner]
            );
        }
        match self.mode {
            Mode::Drill => "↑/↓ mover   r reprocessar   x deletar   Esc volta".to_string(),
            Mode::Summary => {
                let base = "d/w/m/y   ←/→   ↑/↓   Enter detalhes   X apagar categoria   q sair";
                if self.unprocessed_count > 0 {
                    format!("{base}   r reprocessar ({})", self.unprocessed_count)
                } else {
                    base.to_string()
                }
            }
        }
    }

    fn draw_summary(&mut self, frame: &mut Frame, area: Rect) {
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

    fn draw_drill(&mut self, frame: &mut Frame, area: Rect) {
        let title = self.drill_title();
        let entries = self.drill_entries();
        let items: Vec<ListItem> = entries.iter().map(|e| drill_item(e)).collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, area, &mut self.drill);
    }

    fn drill_title(&self) -> String {
        let category = self.selected_category().unwrap_or("");
        format!(" {category} · {} ", self.period.label())
    }
}

/// Reparse entries through Claude, in place. `ids = None` reprocesses every
/// offline entry; `Some(ids)` reprocesses those specific rows. Returns how many
/// succeeded; per-entry parse failures are skipped.
fn reprocess(path: &Path, parser: &dyn ActionParser, ids: Option<Vec<i64>>) -> Result<usize> {
    let store = Store::open(path)?;
    let targets = match ids {
        None => store.unprocessed()?,
        Some(ids) => {
            let mut found = Vec::new();
            for id in ids {
                if let Some(entry) = store.get(id)? {
                    found.push(entry);
                }
            }
            found
        }
    };
    let mut done = 0;
    for entry in targets {
        if let Ok(action) = parser.parse(&entry.raw_text) {
            store.update_processed(entry.id, &action.normalized())?;
            done += 1;
        }
    }
    Ok(done)
}

/// Clamp `cur + delta` into `[0, len)`; `None` if the list is empty or unchanged.
fn move_within(cur: usize, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let next = (cur as isize + delta).clamp(0, len as isize - 1) as usize;
    if next == cur {
        None
    } else {
        Some(next)
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
