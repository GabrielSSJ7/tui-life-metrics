use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::models::ParsedAction;
use crate::parser::ActionParser;
use crate::store::Store;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TICK: Duration = Duration::from_millis(90);

/// Open the capture window: type an action, Claude parses it, result is saved.
pub fn run(store: Store, parser: Arc<dyn ActionParser>) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = Capture::new(store, parser).run(&mut terminal);
    ratatui::restore();
    result
}

/// What the capture screen is currently doing.
enum Phase {
    Editing,
    Parsing {
        rx: Receiver<Result<ParsedAction>>,
        raw: String,
    },
    Done {
        message: String,
        ok: bool,
    },
}

struct Capture {
    store: Store,
    parser: Arc<dyn ActionParser>,
    input: String,
    phase: Phase,
    spinner: usize,
    quit: bool,
}

impl Capture {
    fn new(store: Store, parser: Arc<dyn ActionParser>) -> Self {
        Self {
            store,
            parser,
            input: String::new(),
            phase: Phase::Editing,
            spinner: 0,
            quit: false,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            self.pump()?;
        }
        Ok(())
    }

    /// One event/tick cycle: advance the spinner, poll input, harvest results.
    fn pump(&mut self) -> Result<()> {
        if event::poll(TICK)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.on_key(key.code)?;
                }
            }
        }
        self.spinner = (self.spinner + 1) % SPINNER.len();
        self.poll_parse()?;
        Ok(())
    }

    fn on_key(&mut self, code: KeyCode) -> Result<()> {
        match &self.phase {
            Phase::Editing => self.on_edit_key(code),
            Phase::Done { .. } => self.on_done_key(code),
            Phase::Parsing { .. } => Ok(()),
        }
    }

    fn on_edit_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Esc => self.quit = true,
            KeyCode::Enter if !self.input.trim().is_empty() => self.submit(),
            KeyCode::Char(c) => self.input.push(c),
            KeyCode::Backspace => {
                self.input.pop();
            }
            _ => {}
        }
        Ok(())
    }

    fn on_done_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Enter => {
                self.input.clear();
                self.phase = Phase::Editing;
            }
            KeyCode::Esc => self.quit = true,
            _ => {}
        }
        Ok(())
    }

    /// Kick off parsing on a background thread so the spinner keeps animating.
    fn submit(&mut self) {
        let raw = self.input.trim().to_string();
        let (tx, rx) = mpsc::channel();
        let parser = Arc::clone(&self.parser);
        let sentence = raw.clone();
        thread::spawn(move || {
            let _ = tx.send(parser.parse(&sentence));
        });
        self.phase = Phase::Parsing { rx, raw };
    }

    /// If the background parse finished, persist the outcome.
    fn poll_parse(&mut self) -> Result<()> {
        let Phase::Parsing { rx, raw } = &self.phase else {
            return Ok(());
        };
        let Ok(result) = rx.try_recv() else {
            return Ok(());
        };
        let raw = raw.clone();
        self.phase = match result {
            Ok(action) => self.save_parsed(&raw, action)?,
            Err(err) => self.save_offline(&raw, err)?,
        };
        Ok(())
    }

    fn save_parsed(&self, raw: &str, action: ParsedAction) -> Result<Phase> {
        self.store.insert(raw, &action)?;
        let attrs: Vec<String> = action
            .attributes
            .iter()
            .map(|(k, v)| format!("{k}={}", v.display()))
            .collect();
        let detail = if attrs.is_empty() {
            String::new()
        } else {
            format!("  ({})", attrs.join(", "))
        };
        let message = format!("✓ {} · {}{}", action.category, action.occurred_on, detail);
        Ok(Phase::Done { message, ok: true })
    }

    fn save_offline(&self, raw: &str, err: anyhow::Error) -> Result<Phase> {
        let today = Local::now().date_naive();
        self.store.insert_unprocessed(raw, today)?;
        let message = format!("⚠ Claude indisponível ({err}). Salvo p/ reprocessar depois.");
        Ok(Phase::Done { message, ok: false })
    }

    fn draw(&self, frame: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(frame.area());

        let title = Paragraph::new("Registrar ação")
            .style(Style::default().add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, areas[0]);

        let input = Paragraph::new(self.input.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" o que você fez? "),
        );
        frame.render_widget(input, areas[1]);

        frame.render_widget(self.status_line(), areas[2]);
    }

    fn status_line(&self) -> Paragraph<'_> {
        let line = match &self.phase {
            Phase::Editing => Line::from("Enter: registrar   Esc: sair"),
            Phase::Parsing { .. } => Line::from(vec![
                Span::styled(SPINNER[self.spinner], Style::default().fg(Color::Cyan)),
                Span::raw(" processando com Claude…"),
            ]),
            Phase::Done { message, ok } => {
                let color = if *ok { Color::Green } else { Color::Yellow };
                Line::from(vec![
                    Span::styled(message.clone(), Style::default().fg(color)),
                    Span::raw("   [Enter: nova   Esc: sair]"),
                ])
            }
        };
        Paragraph::new(line).block(Block::default().borders(Borders::ALL).title(" status "))
    }
}
