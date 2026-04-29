//! Minimal TUI for the orchyn fixer loop.
//!
//! Subscribes to `AgentEvent`s from `FixerAgent::subscribe()` and renders three
//! panes: status bar, action feed (left), evidence summary (right). Designed
//! to make the human-parity harness *visible* — every tool call the LLM picks
//! shows up in the feed, evidence updates show up on the right.

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use orchyn_core::{DiagnosticEvent, EvidenceBundle, LogLevel};
use orchyn_fixer_agent::AgentEvent;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::time::Duration;
use tokio::sync::broadcast;

const FEED_CAP: usize = 200;

struct UiState {
    step: (usize, usize),
    feed: VecDeque<FeedLine>,
    evidence: Option<EvidenceBundle>,
    finished: Option<String>,
    model_id: String,
    initial_sha: Option<String>,
}

struct FeedLine {
    kind: FeedKind,
    text: String,
}

enum FeedKind {
    Step,
    Call,
    Ok,
    Err,
    Notice,
    Finish,
}

impl UiState {
    fn new(model_id: String, initial_sha: Option<String>) -> Self {
        Self {
            step: (0, 0),
            feed: VecDeque::with_capacity(FEED_CAP),
            evidence: None,
            finished: None,
            model_id,
            initial_sha,
        }
    }

    fn push(&mut self, line: FeedLine) {
        if self.feed.len() == FEED_CAP {
            self.feed.pop_front();
        }
        self.feed.push_back(line);
    }

    fn handle(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Step { n, max } => {
                self.step = (n, max);
                self.push(FeedLine {
                    kind: FeedKind::Step,
                    text: format!("── step {}/{} ─────", n, max),
                });
            }
            AgentEvent::ToolCall { name, args } => {
                let arg_summary = compact_args(&args);
                self.push(FeedLine {
                    kind: FeedKind::Call,
                    text: format!("→ {} {}", name, arg_summary),
                });
            }
            AgentEvent::ToolResult { name, ok, summary } => {
                let kind = if ok { FeedKind::Ok } else { FeedKind::Err };
                let glyph = if ok { "✓" } else { "✗" };
                self.push(FeedLine {
                    kind,
                    text: format!("  {} {}: {}", glyph, name, summary),
                });
            }
            AgentEvent::Evidence(b) => {
                self.evidence = Some(*b);
            }
            AgentEvent::Notice(msg) => {
                self.push(FeedLine {
                    kind: FeedKind::Notice,
                    text: format!("• {}", msg),
                });
            }
            AgentEvent::Finish { summary } => {
                self.finished = Some(summary.clone());
                self.push(FeedLine {
                    kind: FeedKind::Finish,
                    text: format!("◆ finished: {}", summary),
                });
            }
        }
    }
}

fn compact_args(args: &serde_json::Value) -> String {
    let s = args.to_string();
    let trimmed: String = s.chars().take(80).collect();
    if trimmed.len() < s.len() {
        format!("{}…", trimmed)
    } else {
        trimmed
    }
}

/// Run the TUI until the agent emits `Finish` (or the user presses q).
pub async fn run_tui(
    mut events: broadcast::Receiver<AgentEvent>,
    model_id: String,
    initial_sha: Option<String>,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = UiState::new(model_id, initial_sha);

    let result = run_loop(&mut terminal, &mut state, &mut events).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut UiState,
    events: &mut broadcast::Receiver<AgentEvent>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| draw(f, state))?;

        // Drain any pending agent events without blocking.
        loop {
            match events.try_recv() {
                Ok(ev) => state.handle(ev),
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(broadcast::error::TryRecvError::Closed) => {
                    if state.finished.is_none() {
                        state.finished = Some("(agent disconnected)".to_string());
                    }
                    break;
                }
            }
        }

        // Poll the keyboard with a short timeout so the UI stays responsive.
        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        _ => {}
                    }
                }
            }
        }

        if state.finished.is_some() {
            // Allow the user a moment to read; quit on next q/esc.
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press
                        && matches!(key.code, KeyCode::Char('q') | KeyCode::Enter | KeyCode::Esc)
                    {
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn draw(f: &mut Frame, state: &UiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(f.area());

    draw_header(f, chunks[0], state);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[1]);

    draw_feed(f, body[0], state);
    draw_evidence(f, body[1], state);
    draw_footer(f, chunks[2], state);
}

fn draw_header(f: &mut Frame, area: Rect, state: &UiState) {
    let (n, max) = state.step;
    let _progress = if max == 0 {
        "ready".to_string()
    } else {
        format!("step {}/{}", n, max)
    };
    let sha = state.initial_sha.as_deref().unwrap_or("<no-git>");
    let title = format!(
        " ORCHYN  ·  Qwen 3.5 9B ({})  ·   HEAD@{} ",
        state.model_id,
        &sha[..sha.len().min(8)]
    );
    let header = Paragraph::new(Line::from(vec![Span::styled(
        title,
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]))
    .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, area);
}

fn draw_feed(f: &mut Frame, area: Rect, state: &UiState) {
    let items: Vec<ListItem> = state
        .feed
        .iter()
        .map(|line| {
            let style = match line.kind {
                FeedKind::Step => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                FeedKind::Call => Style::default().fg(Color::Yellow),
                FeedKind::Ok => Style::default().fg(Color::Green),
                FeedKind::Err => Style::default().fg(Color::Red),
                FeedKind::Notice => Style::default().fg(Color::Magenta),
                FeedKind::Finish => Style::default()
                    .fg(Color::White)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            };
            ListItem::new(Span::styled(line.text.clone(), style))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" agent actions "),
    );
    f.render_widget(list, area);
}

fn draw_evidence(f: &mut Frame, area: Rect, state: &UiState) {
    let lines = match &state.evidence {
        None => vec![Line::from("no evidence yet")],
        Some(b) => render_evidence_lines(b),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" human-parity evidence ");
    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn render_evidence_lines(b: &EvidenceBundle) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if let Some(goal) = &b.goal {
        lines.push(Line::from(vec![
            Span::styled("goal: ", Style::default().fg(Color::Gray)),
            Span::raw(goal.clone()),
        ]));
    }

    // Screenshot paths
    if !b.screenshot_paths.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("visual  ", Style::default().fg(Color::Blue)),
            Span::raw(format!("{} shots", b.screenshot_paths.len())),
        ]));
    }

    // Diagnostics
    if !b.diagnostics.is_empty() {
        let console_errs = b
            .diagnostics
            .iter()
            .filter(|d| {
                matches!(
                    d,
                    DiagnosticEvent::Console {
                        level: LogLevel::Error,
                        ..
                    }
                )
            })
            .count();
        lines.push(Line::from(vec![
            Span::styled("console ", Style::default().fg(Color::Blue)),
            Span::styled(
                format!("{} err", console_errs),
                color_for_count(console_errs),
            ),
        ]));
    }

    // Network
    if !b.network_events.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("network ", Style::default().fg(Color::Blue)),
            Span::raw(format!("{} events", b.network_events.len())),
        ]));
    }

    // Findings
    if !b.findings.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "findings",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        for f in b.findings.iter().take(5) {
            lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Green)),
                Span::raw(truncate(&f.description, 50)),
            ]));
        }
    }

    lines
}

fn color_for_count(n: usize) -> Style {
    if n == 0 {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

fn draw_footer(f: &mut Frame, area: Rect, state: &UiState) {
    let msg = match &state.finished {
        Some(s) => format!(" finished — {}  ·  press q/enter to exit ", s),
        None => " q: quit ".to_string(),
    };
    let p = Paragraph::new(msg)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(p, area);
}
