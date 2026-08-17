//! Fuzzy-filterable picker over a match list, ported from the original
//! zellij-zextract plugin's `ratatui` UI (`State::render_input`,
//! `render_list`, `refilter`, `handle_key*` in the original `main.rs`).
//!
//! The original renders through a hand-rolled Buffer→ANSI emitter
//! (`render.rs`) because Zellij's WASM host couldn't use `crossterm`
//! directly. Herdr plugins are native processes with a real PTY, so
//! this port uses `ratatui`'s standard `CrosstermBackend` instead —
//! simpler, and gets terminal-resize/cursor handling for free.
//!
//! Scope: fuzzy-filter, `#type` include/exclude tokens, navigate, and
//! select-with-verb (Enter fires the type's default verb; Ctrl+letter
//! fires a specific one via `actions::Verb`). No multi-select, preview
//! pane, or config-driven colors yet (Phase 5).

pub mod fuzzy;
pub mod query;

use std::collections::HashSet;
use std::io;

use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

use crate::actions::{self, Verb};
use crate::matcher::{type_priority_bonus, Match, MatchType, TYPE_PRIORITY};
use fuzzy::{FuzzyEngine, ScoredMatch};
use query::ParsedQuery;

const MUTED: Color = Color::DarkGray;
const HIGHLIGHT: Color = Color::Yellow;
const CURSOR_BG: Color = Color::Blue;
const CURSOR_FG: Color = Color::Black;

/// Per-type list/tag colors, matching the original plugin's default
/// theme (`ColorsConfig::default`) — hardcoded here until Phase 5 makes
/// this configurable.
fn color_for_type(ty: MatchType) -> Color {
    match ty {
        MatchType::Url => Color::Blue,
        MatchType::File => Color::Green,
        MatchType::Diagnostic => Color::LightRed,
        MatchType::Git => Color::Yellow,
        MatchType::Sha => Color::Yellow,
        MatchType::Ipv4 => Color::Cyan,
        MatchType::Ipv6 => Color::Cyan,
        MatchType::Uuid => Color::Magenta,
        MatchType::QuotedString => Color::Gray,
        MatchType::Command => Color::LightMagenta,
        MatchType::Secret => Color::LightRed,
    }
}

fn color_for_tag(tag: &str) -> Color {
    TYPE_PRIORITY
        .iter()
        .find(|t| t.tag() == tag)
        .map(|&t| color_for_type(t))
        .unwrap_or(Color::Gray)
}

struct State {
    matches: Vec<Match>,
    query: String,
    parsed_query: ParsedQuery,
    fuzzy: FuzzyEngine,
    filtered: Vec<ScoredMatch>,
    list_state: ListState,
    last_rows: usize,
}

impl State {
    fn new(matches: Vec<Match>) -> Self {
        let mut state = Self {
            matches,
            query: String::new(),
            parsed_query: ParsedQuery::default(),
            fuzzy: FuzzyEngine::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            last_rows: 24,
        };
        state.refilter();
        state
    }

    /// Re-run `#type` filtering + fuzzy scoring over `self.matches`,
    /// preserving the highlighted row across the change when possible.
    /// Ported from the original `State::refilter`.
    fn refilter(&mut self) {
        let prev_selected_match_idx = self
            .list_state
            .selected()
            .and_then(|i| self.filtered.get(i))
            .map(|s| s.index);

        let tags: Vec<&str> = TYPE_PRIORITY.iter().map(|t| t.tag()).collect();
        self.parsed_query = query::parse_query(&self.query, &tags);
        let parsed = &self.parsed_query;

        let allowed_indices: Vec<usize> = self
            .matches
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                let tag = m.ty.tag();
                let include_ok =
                    parsed.includes.is_empty() || parsed.includes.iter().any(|t| t == tag);
                let exclude_ok = !parsed.excludes.iter().any(|t| t == tag);
                include_ok && exclude_ok
            })
            .map(|(i, _)| i)
            .collect();

        let allowed_displays: Vec<&str> = allowed_indices
            .iter()
            .map(|&i| self.matches[i].display.as_str())
            .collect();

        let matches = &self.matches;
        let idx_map = &allowed_indices;
        let scored = self
            .fuzzy
            .filter_with_bonus(&parsed.fuzzy, &allowed_displays, |i| {
                idx_map
                    .get(i)
                    .and_then(|&mi| matches.get(mi))
                    .map(|m| type_priority_bonus(m.ty))
                    .unwrap_or(0)
            });

        self.filtered = scored
            .into_iter()
            .filter_map(|s| {
                allowed_indices.get(s.index).map(|&mi| ScoredMatch {
                    index: mi,
                    score: s.score,
                    indices: s.indices,
                })
            })
            .collect();

        let new_selection = if let Some(prev) = prev_selected_match_idx {
            self.filtered
                .iter()
                .position(|s| s.index == prev)
                .unwrap_or(0)
        } else {
            0
        };
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(new_selection));
        }
    }

    fn current_match(&self) -> Option<&Match> {
        let i = self.list_state.selected()?;
        let scored = self.filtered.get(i)?;
        self.matches.get(scored.index)
    }

    /// Visible list rows, for PageUp/PageDown step size. Falls back to
    /// a sane default before the first render reports the real size.
    fn list_page_size(&self) -> usize {
        self.last_rows.saturating_sub(5).max(1)
    }
}

/// What the user did with the picker: picked a match with a specific
/// verb (Enter fires the type's default; Ctrl+<letter> fires a
/// specific one, when allowed for that match's type), or cancelled.
pub enum PickerResult {
    Selected(Match, Verb),
    Cancelled,
}

/// Run the picker over `matches` as a fullscreen terminal UI.
pub fn run(matches: Vec<Match>) -> io::Result<PickerResult> {
    let mut state = State::new(matches);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    // ratatui's diff renderer assumes it starts from a blank terminal;
    // without this, cells that happen to render as blank in the first
    // frame don't get force-written, leaving old scrollback content
    // showing through around/behind the picker.
    terminal.clear()?;

    let result = run_loop(&mut terminal, &mut state);

    disable_raw_mode()?;
    let _ = crossterm::execute!(terminal.backend_mut(), crossterm::cursor::Show);
    let _ = terminal.clear();

    result
}

/// If `verb` is allowed for the currently-highlighted match, returns
/// the Selected result for it; otherwise `None` (key ignored).
fn select_with_verb(state: &State, verb: Verb) -> Option<PickerResult> {
    let m = state.current_match()?;
    if !actions::is_verb_allowed(m, verb) {
        return None;
    }
    Some(PickerResult::Selected(m.clone(), verb))
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut State,
) -> io::Result<PickerResult> {
    loop {
        terminal.draw(|f| render(f, state))?;
        state.last_rows = terminal.size()?.height as usize;

        let CtEvent::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            let verb = match key.code {
                KeyCode::Char('y') => Some(Verb::CopyRaw),
                KeyCode::Char('i') => Some(Verb::Insert),
                KeyCode::Char('o') => Some(Verb::Open),
                KeyCode::Char('e') => Some(Verb::Edit),
                KeyCode::Char('j') => Some(Verb::Json),
                _ => None,
            };
            if let Some(result) = verb.and_then(|v| select_with_verb(state, v)) {
                return Ok(result);
            }
            continue;
        }
        match key.code {
            KeyCode::Esc => return Ok(PickerResult::Cancelled),
            KeyCode::Enter => {
                if let Some(m) = state.current_match() {
                    let verb = actions::default_verb(m.ty);
                    return Ok(PickerResult::Selected(m.clone(), verb));
                }
            }
            KeyCode::Backspace => {
                if state.query.pop().is_some() {
                    state.refilter();
                }
            }
            KeyCode::Up => {
                let i = state.list_state.selected().unwrap_or(0);
                if i > 0 {
                    state.list_state.select(Some(i - 1));
                }
            }
            KeyCode::Down => {
                let i = state.list_state.selected().unwrap_or(0);
                if !state.filtered.is_empty() && i + 1 < state.filtered.len() {
                    state.list_state.select(Some(i + 1));
                }
            }
            KeyCode::PageUp => {
                let page = state.list_page_size();
                let i = state.list_state.selected().unwrap_or(0);
                state.list_state.select(Some(i.saturating_sub(page)));
            }
            KeyCode::PageDown => {
                let page = state.list_page_size();
                let i = state.list_state.selected().unwrap_or(0);
                if !state.filtered.is_empty() {
                    let next = (i + page).min(state.filtered.len() - 1);
                    state.list_state.select(Some(next));
                }
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                state.query.push(c);
                state.refilter();
            }
            _ => {}
        }
    }
}

fn render(frame: &mut Frame, state: &mut State) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);
    render_input(frame, chunks[0], state);
    render_list(frame, chunks[1], state);
}

fn render_input(frame: &mut Frame, area: Rect, state: &State) {
    let mut spans = vec![
        Span::styled(
            "▍ ",
            Style::default().fg(CURSOR_BG).add_modifier(Modifier::BOLD),
        ),
        Span::raw(state.query.clone()),
        Span::styled("█", Style::default().fg(MUTED).add_modifier(Modifier::DIM)),
        Span::raw("   "),
    ];
    for inc in &state.parsed_query.includes {
        spans.push(Span::styled(
            format!("[{inc}]"),
            Style::default()
                .fg(color_for_tag(inc))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }
    for exc in &state.parsed_query.excludes {
        spans.push(Span::styled(
            format!("[-{exc}]"),
            Style::default().fg(MUTED).add_modifier(Modifier::DIM),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        format!("{}/{}", state.filtered.len(), state.matches.len()),
        Style::default().fg(MUTED),
    ));
    let p = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("zextract  (Esc cancel, Enter select, #type filter)"),
    );
    frame.render_widget(p, area);
}

fn render_list(frame: &mut Frame, area: Rect, state: &mut State) {
    if state.matches.is_empty() {
        let p = Paragraph::new("No matches in pane scrollback.")
            .style(Style::default().fg(MUTED))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
        return;
    }
    if state.filtered.is_empty() {
        let p = Paragraph::new(format!("No matches for \"{}\"", state.query))
            .style(Style::default().fg(MUTED))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = state
        .filtered
        .iter()
        .filter_map(|s| state.matches.get(s.index).map(|m| (s, m)))
        .map(|(s, m)| {
            let tag_span = Span::styled(
                format!("[{}]  ", m.ty.tag()),
                Style::default().fg(color_for_type(m.ty)),
            );
            let tag_overhead = m.ty.tag().chars().count() + 6; // "[tag]  "
            let avail = (area.width as usize).saturating_sub(tag_overhead);
            let use_middle = matches!(
                m.ty,
                MatchType::Url | MatchType::File | MatchType::Diagnostic | MatchType::Git
            );
            let display = truncate_display(&m.display, avail, use_middle);
            let mut spans = vec![tag_span];
            spans.extend(highlight_spans(&display, &s.indices, HIGHLIGHT));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(CURSOR_BG)
                .fg(CURSOR_FG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, area, &mut state.list_state);
}

/// Ported from the original `main.rs::truncate_display` verbatim.
fn truncate_display(s: &str, max_chars: usize, middle: bool) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    if max_chars < 3 {
        return "…".to_string();
    }
    if middle {
        let half = (max_chars - 1) / 2;
        let left: String = chars[..half].iter().collect();
        let right: String = chars[chars.len() - (max_chars - 1 - half)..]
            .iter()
            .collect();
        format!("{left}…{right}")
    } else {
        let truncated: String = chars[..max_chars - 1].iter().collect();
        format!("{truncated}…")
    }
}

/// Ported from the original `main.rs::highlight_spans` verbatim.
fn highlight_spans(display: &str, indices: &[u32], color: Color) -> Vec<Span<'static>> {
    if indices.is_empty() {
        return vec![Span::raw(display.to_string())];
    }
    let hi: HashSet<u32> = indices.iter().copied().collect();
    let highlight = Style::default().fg(color).add_modifier(Modifier::BOLD);

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut current_hi = false;

    for (i, ch) in display.chars().enumerate() {
        let this_hi = hi.contains(&(i as u32));
        if this_hi != current_hi && !current.is_empty() {
            let style = if current_hi {
                highlight
            } else {
                Style::default()
            };
            spans.push(Span::styled(std::mem::take(&mut current), style));
        }
        current_hi = this_hi;
        current.push(ch);
    }
    if !current.is_empty() {
        let style = if current_hi {
            highlight
        } else {
            Style::default()
        };
        spans.push(Span::styled(current, style));
    }
    spans
}
