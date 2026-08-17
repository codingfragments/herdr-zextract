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
//! Scope: fuzzy-filter, `#type` include/exclude tokens, navigate,
//! Input/List modes (`Tab` toggles - Input types into the query, List
//! fires bare-letter verbs), multi-select (`Space`/`Ctrl-A`/`Ctrl-D`)
//! with batch dispatch, a dedicated footer/banner row, a config-driven
//! `[colors]` theme (Phase 8), live `Ctrl-G` cycling through every
//! configured grab profile (re-captures + re-extracts in place via
//! [`crate::grab::GrabCycler`], owned by [`State`] for the session), and
//! a `p`/`Ctrl-P` preview split (Phase 9) showing ±3 lines of context
//! around the highlighted match ([`preview::context_lines`]), sized by
//! `[ui].preview_open_width` - see that module and
//! `doc/config-reference.md` for why this port splits its own render
//! area instead of resizing the real popup pane like the original does.

pub mod fuzzy;
pub mod preview;
pub mod query;

use std::collections::{HashMap, HashSet};
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

/// Resolved UI palette, built once from `[colors]` at picker startup.
/// Every slot has an ANSI-palette default matching the original's
/// `ColorsConfig::default` exactly; a config override replaces that
/// slot's color outright, matched loosely against the original.
struct Theme {
    muted: Color,
    accent: Color,
    cursor_bg: Color,
    cursor_fg: Color,
    highlight: Color,
    error: Color,
    fallback_type: Color,
    type_url: Color,
    type_file: Color,
    type_diag: Color,
    type_git: Color,
    type_sha: Color,
    type_ipv4: Color,
    type_ipv6: Color,
    type_uuid: Color,
    type_quoted: Color,
    type_command: Color,
    type_secret: Color,
}

impl Theme {
    fn resolve(colors: &crate::config::ColorsConfig) -> Self {
        let c = |over: &Option<String>, default: Color| -> Color {
            over.as_deref().and_then(parse_color).unwrap_or(default)
        };
        Self {
            muted: c(&colors.muted, Color::DarkGray),
            accent: c(&colors.accent, Color::Cyan),
            cursor_bg: c(&colors.cursor_bg, Color::Blue),
            cursor_fg: c(&colors.cursor_fg, Color::Black),
            highlight: c(&colors.highlight, Color::Yellow),
            error: c(&colors.error, Color::LightRed),
            fallback_type: c(&colors.fallback_type, Color::Gray),
            type_url: c(&colors.type_url, Color::Blue),
            type_file: c(&colors.type_file, Color::Green),
            type_diag: c(&colors.type_diag, Color::LightRed),
            type_git: c(&colors.type_git, Color::Yellow),
            type_sha: c(&colors.type_sha, Color::Yellow),
            type_ipv4: c(&colors.type_ipv4, Color::Cyan),
            type_ipv6: c(&colors.type_ipv6, Color::Cyan),
            type_uuid: c(&colors.type_uuid, Color::Magenta),
            type_quoted: c(&colors.type_quoted, Color::Gray),
            type_command: c(&colors.type_command, Color::LightMagenta),
            type_secret: c(&colors.type_secret, Color::LightRed),
        }
    }

    fn color_for_type(&self, ty: MatchType) -> Color {
        match ty {
            MatchType::Url => self.type_url,
            MatchType::File => self.type_file,
            MatchType::Diagnostic => self.type_diag,
            MatchType::Git => self.type_git,
            MatchType::Sha => self.type_sha,
            MatchType::Ipv4 => self.type_ipv4,
            MatchType::Ipv6 => self.type_ipv6,
            MatchType::Uuid => self.type_uuid,
            MatchType::QuotedString => self.type_quoted,
            MatchType::Command => self.type_command,
            MatchType::Secret => self.type_secret,
        }
    }

    /// Color for a `#tag` - a built-in type's own slot, or
    /// `fallback_type` for a custom pattern name with no dedicated
    /// slot (custom patterns are always tagged by name, not type).
    fn color_for_tag(&self, tag: &str) -> Color {
        TYPE_PRIORITY
            .iter()
            .find(|t| t.tag() == tag)
            .map(|&t| self.color_for_type(t))
            .unwrap_or(self.fallback_type)
    }
}

/// Parses a `[colors]` value: an ANSI name, `#rrggbb` hex, or
/// `rgb(r,g,b)`. Returns `None` on anything unrecognized, so a typo
/// falls back to that slot's built-in default rather than failing.
fn parse_color(s: &str) -> Option<Color> {
    match s {
        "black" => Some(Color::Black),
        "dark_gray" => Some(Color::DarkGray),
        "gray" => Some(Color::Gray),
        "white" => Some(Color::White),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "light_red" => Some(Color::LightRed),
        "light_green" => Some(Color::LightGreen),
        "light_yellow" => Some(Color::LightYellow),
        "light_blue" => Some(Color::LightBlue),
        "light_magenta" => Some(Color::LightMagenta),
        "light_cyan" => Some(Color::LightCyan),
        _ => {
            if let Some(hex) = s.strip_prefix('#') {
                parse_hex(hex)
            } else if let Some(inner) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
                parse_rgb(inner)
            } else {
                None
            }
        }
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn parse_rgb(inner: &str) -> Option<Color> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].parse::<u8>().ok()?;
    let g = parts[1].parse::<u8>().ok()?;
    let b = parts[2].parse::<u8>().ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Input mode types into the query; List mode fires bare-letter verbs
/// instead. `Tab` toggles. Ported from the original's `Mode` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Input,
    List,
}

struct State {
    matches: Vec<Match>,
    /// Custom pattern names, appended to `TYPE_PRIORITY`'s tags when
    /// resolving `#name` filter tokens - lets a custom pattern be
    /// filtered by its own configured name, not just its underlying type.
    custom_tags: Vec<String>,
    query: String,
    parsed_query: ParsedQuery,
    fuzzy: FuzzyEngine,
    filtered: Vec<ScoredMatch>,
    list_state: ListState,
    last_rows: usize,
    /// True when `$HERDR_PLUGIN_CONFIG_DIR/config.toml` doesn't exist
    /// yet - shows the config-missing banner and lets `Ctrl-W` write a
    /// starter file. Cleared after a successful write.
    config_missing: bool,
    /// User dismissed the config-missing banner (`Ctrl-X`) without
    /// writing a config. `config_missing` stays true (the file still
    /// doesn't exist) but the banner stops showing.
    config_missing_dismissed: bool,
    /// Transient status line (text, is_error) - cleared on the next
    /// keystroke. `is_error` picks `[colors].error` vs `.highlight`
    /// when rendering, matching the original's "warning label" vs.
    /// plain status-message use of those two slots.
    message: Option<(String, bool)>,
    mode: Mode,
    /// Multi-selection: indices into `self.matches`, stable across
    /// filter changes (a row stays selected even when filtered out,
    /// and reappears already-selected when the filter brings it back).
    selected: HashSet<usize>,
    theme: Theme,
    /// Cloned once at startup - `[types.<tag>]`/`[limits]` drive the
    /// footer's allowed-verb hints and `try_fire`'s batch-cap checks.
    config: crate::config::Config,
    /// `Ctrl-G` cycles through this - re-captures and re-extracts with
    /// the next grab profile, refreshing `matches` in place.
    grab_cycler: crate::grab::GrabCycler,
    /// Every contributing pane's full captured text, keyed by pane id -
    /// the preview pane slices `context_lines` out of this using a
    /// match's `__pane_id` field. Replaced wholesale on every regrab.
    pane_texts: HashMap<String, String>,
    /// `p` (List mode) / `Ctrl-P` (either mode) toggles this.
    preview_open: bool,
}

impl State {
    fn new(launch: LaunchArgs) -> Self {
        let LaunchArgs {
            matches,
            pane_texts,
            custom_tags,
            config_missing,
            config,
            initial_query,
            preview_open,
            grab_cycler,
        } = launch;
        let mut state = Self {
            matches,
            custom_tags: custom_tags.to_vec(),
            query: initial_query.to_string(),
            parsed_query: ParsedQuery::default(),
            fuzzy: FuzzyEngine::new(),
            filtered: Vec::new(),
            list_state: ListState::default(),
            last_rows: 24,
            config_missing,
            config_missing_dismissed: false,
            message: None,
            mode: Mode::Input,
            selected: HashSet::new(),
            theme: Theme::resolve(&config.colors),
            config: config.clone(),
            grab_cycler,
            pane_texts,
            preview_open,
        };
        state.refilter();
        state
    }

    /// Re-capture and re-extract with the next grab profile in the
    /// cycle (`Ctrl-G`), replacing `self.matches`/`self.pane_texts` and
    /// refiltering on success. The multi-selection doesn't survive - a
    /// regrab can wholly change which matches exist, so stale indices
    /// into the old `matches` vector would silently select the wrong
    /// rows.
    fn cycle_grab(&mut self) {
        match self.grab_cycler.cycle_next() {
            Ok(result) => {
                self.matches = result.matches;
                self.pane_texts = result.pane_texts;
                self.selected.clear();
                self.refilter();
                self.message = Some((
                    format!(
                        "grab: {} ({} matches)",
                        self.grab_cycler.current_name(),
                        self.matches.len()
                    ),
                    false,
                ));
            }
            Err(e) => self.message = Some((format!("regrab failed: {e}"), true)),
        }
    }

    /// Toggle the highlighted row's membership in the multi-selection.
    fn toggle_select_current(&mut self) {
        let Some(idx) = self.current_match_index() else {
            return;
        };
        if !self.selected.insert(idx) {
            self.selected.remove(&idx);
        }
    }

    /// Select every match currently visible in the filtered list.
    fn select_all_visible(&mut self) {
        for s in &self.filtered {
            self.selected.insert(s.index);
        }
    }

    fn deselect_all(&mut self) {
        self.selected.clear();
    }

    /// The `Match`es to act on: the multi-selection if non-empty,
    /// otherwise the highlighted row alone (empty if there's no
    /// selection cursor either). Ported from the original's
    /// `effective_targets`, preserving the filtered list's recency
    /// order in the result.
    fn effective_targets(&self) -> Vec<&Match> {
        if !self.selected.is_empty() {
            return self
                .filtered
                .iter()
                .filter(|s| self.selected.contains(&s.index))
                .filter_map(|s| self.matches.get(s.index))
                .collect();
        }
        self.current_match().into_iter().collect()
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

        let mut tags: Vec<&str> = TYPE_PRIORITY.iter().map(|t| t.tag()).collect();
        tags.extend(self.custom_tags.iter().map(String::as_str));
        self.parsed_query = query::parse_query(&self.query, &tags);
        let parsed = &self.parsed_query;

        let allowed_indices: Vec<usize> = self
            .matches
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                let tag = m.effective_tag();
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

    /// Index into `self.matches` for the currently-highlighted row.
    fn current_match_index(&self) -> Option<usize> {
        let i = self.list_state.selected()?;
        Some(self.filtered.get(i)?.index)
    }

    /// Visible list rows, for PageUp/PageDown step size. Falls back to
    /// a sane default before the first render reports the real size.
    fn list_page_size(&self) -> usize {
        self.last_rows.saturating_sub(5).max(1)
    }
}

/// What the user did with the picker: fired a verb over one or more
/// matches (Enter fires the type's default; a List-mode bare letter or
/// a Ctrl+letter universal shortcut fires a specific one, over the
/// multi-selection if non-empty or the highlighted row otherwise), or
/// cancelled.
pub enum PickerResult {
    Selected(Vec<Match>, Verb),
    Cancelled,
}

/// Bundled [`run`] parameters - plain positional args would put this
/// past clippy's `too_many_arguments` threshold.
pub struct LaunchArgs<'a> {
    pub matches: Vec<Match>,
    /// Every contributing pane's full captured text, keyed by pane id -
    /// see [`State::pane_texts`].
    pub pane_texts: HashMap<String, String>,
    /// Configured names of any custom patterns, so they resolve as
    /// `#name` filter tokens alongside the built-in types.
    pub custom_tags: &'a [String],
    /// Shows the `Ctrl-W` "write starter config" hint.
    pub config_missing: bool,
    pub config: &'a crate::config::Config,
    /// Pre-fills the filter (e.g. `"#url"` for a per-keybind URL-only
    /// picker).
    pub initial_query: &'a str,
    /// Launch-time state of the `p`/`Ctrl-P` preview split.
    pub preview_open: bool,
    pub grab_cycler: crate::grab::GrabCycler,
}

/// Run the picker as a fullscreen terminal UI - see [`LaunchArgs`] for
/// what each field controls.
pub fn run(launch: LaunchArgs) -> io::Result<PickerResult> {
    let mut state = State::new(launch);

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

/// If firing `verb` over the current effective targets (the
/// multi-selection if non-empty, else the highlighted row) is
/// allowed, returns the `Selected` result to close the picker with.
/// Otherwise sets `state.message` to the rejection reason (original's
/// "loud-reject if zero allowed" / per-verb-cap rules) and returns
/// `None` so the caller keeps the picker open.
fn try_fire(state: &mut State, verb: Verb) -> Option<PickerResult> {
    let targets = state.effective_targets();
    if targets.is_empty() {
        return None;
    }
    match actions::plan_batch(verb, &targets, &state.config) {
        Ok(()) => Some(PickerResult::Selected(
            targets.into_iter().cloned().collect(),
            verb,
        )),
        Err(msg) => {
            state.message = Some((msg, true));
            None
        }
    }
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
        state.message = None; // any keystroke clears the previous status line

        // Universal Ctrl-modified shortcuts, work from either mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('w') => {
                    if state.config_missing {
                        match crate::config::write_default() {
                            Ok(path) => {
                                state.config_missing = false;
                                state.message = Some((
                                    format!("wrote default config to {}", path.display()),
                                    false,
                                ));
                            }
                            Err(e) => {
                                state.message = Some((format!("failed to write config: {e}"), true))
                            }
                        }
                    }
                    continue;
                }
                KeyCode::Char('x') => {
                    state.config_missing_dismissed = true;
                    continue;
                }
                KeyCode::Char('a') => {
                    state.select_all_visible();
                    continue;
                }
                KeyCode::Char('d') => {
                    state.deselect_all();
                    continue;
                }
                KeyCode::Char('g') => {
                    state.cycle_grab();
                    continue;
                }
                KeyCode::Char('p') => {
                    state.preview_open = !state.preview_open;
                    continue;
                }
                _ => {}
            }
            // Note: Ctrl-I is deliberately absent - it's byte-identical
            // to Tab (both 0x09) in terminal protocols, so it would
            // silently toggle the mode instead of firing Insert (found
            // by manual testing). Shift-Enter is the force-insert
            // shortcut instead, matching the original exactly.
            let verb = match key.code {
                KeyCode::Char('y') => Some(Verb::CopyRaw),
                KeyCode::Char('o') => Some(Verb::Open),
                KeyCode::Char('e') => Some(Verb::Edit),
                KeyCode::Char('j') => Some(Verb::Json),
                _ => None,
            };
            if let Some(v) = verb {
                if let Some(result) = try_fire(state, v) {
                    return Ok(result);
                }
            }
            continue;
        }

        // Universal non-modified keys, work from either mode.
        match key.code {
            KeyCode::Esc => return Ok(PickerResult::Cancelled),
            KeyCode::Tab => {
                state.mode = match state.mode {
                    Mode::Input => Mode::List,
                    Mode::List => Mode::Input,
                };
                continue;
            }
            // Shift-Enter forces Insert regardless of the type's
            // default - the original's shortcut for this; also where
            // force-insert lives since Ctrl-I isn't usable (see above).
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if let Some(result) = try_fire(state, Verb::Insert) {
                    return Ok(result);
                }
                continue;
            }
            KeyCode::Enter => {
                if let Some(m) = state.current_match() {
                    let verb = actions::default_verb(m, &state.config);
                    if let Some(result) = try_fire(state, verb) {
                        return Ok(result);
                    }
                }
                continue;
            }
            KeyCode::Up => {
                let i = state.list_state.selected().unwrap_or(0);
                if i > 0 {
                    state.list_state.select(Some(i - 1));
                }
                continue;
            }
            KeyCode::Down => {
                let i = state.list_state.selected().unwrap_or(0);
                if !state.filtered.is_empty() && i + 1 < state.filtered.len() {
                    state.list_state.select(Some(i + 1));
                }
                continue;
            }
            KeyCode::PageUp => {
                let page = state.list_page_size();
                let i = state.list_state.selected().unwrap_or(0);
                state.list_state.select(Some(i.saturating_sub(page)));
                continue;
            }
            KeyCode::PageDown => {
                let page = state.list_page_size();
                let i = state.list_state.selected().unwrap_or(0);
                if !state.filtered.is_empty() {
                    let next = (i + page).min(state.filtered.len() - 1);
                    state.list_state.select(Some(next));
                }
                continue;
            }
            _ => {}
        }

        // Mode-specific: Input types into the query; List fires verbs.
        match state.mode {
            Mode::Input => match key.code {
                KeyCode::Backspace => {
                    if state.query.pop().is_some() {
                        state.refilter();
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
            },
            Mode::List => {
                if key.code == KeyCode::Char(' ') {
                    state.toggle_select_current();
                } else if key.code == KeyCode::Char('p') {
                    state.preview_open = !state.preview_open;
                } else if let KeyCode::Char(c) = key.code {
                    if let Some(v) = actions::verb_from_char(c) {
                        if let Some(result) = try_fire(state, v) {
                            return Ok(result);
                        }
                    }
                }
            }
        }
    }
}

fn render(frame: &mut Frame, state: &mut State) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(4),
        ])
        .split(area);

    // Grab label gets just the width its own text needs (bordered box,
    // right-aligned); the query input takes every column left over.
    let grab_label = grab_label_text(state);
    let grab_width = grab_label.chars().count() as u16 + 4; // borders(2) + padding(2)
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(grab_width)])
        .split(chunks[0]);

    render_input(frame, top[0], state);
    render_grab_label(frame, top[1], state, &grab_label);

    if state.preview_open {
        // `[ui].preview_open_width` sizes the list column (matching its
        // "popup width while the preview is open" doc wording, ported
        // to an internal split since this port doesn't resize the real
        // OS-level popup pane); the preview takes whatever's left.
        // `preview_closed_width` has no effect - closed means no split
        // at all, so there's nothing for it to size.
        let list_width = width_constraint(&state.config.ui.preview_open_width, 40);
        let middle = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([list_width, Constraint::Min(1)])
            .split(chunks[1]);
        render_list(frame, middle[0], state);
        render_preview(frame, middle[1], state);
    } else {
        render_list(frame, chunks[1], state);
    }

    render_footer_or_banner(frame, chunks[2], state);
}

/// Parses a `[ui].preview_*_width`-style value: `"90%"` or a bare cell
/// count (`"120"`). Falls back to `Constraint::Percentage(default_pct)`
/// on anything else, including an out-of-range percentage.
fn width_constraint(s: &str, default_pct: u16) -> Constraint {
    if let Some(pct) = s.trim().strip_suffix('%') {
        return pct
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|&p| p <= 100)
            .map(Constraint::Percentage)
            .unwrap_or(Constraint::Percentage(default_pct));
    }
    s.trim()
        .parse::<u16>()
        .ok()
        .map(Constraint::Length)
        .unwrap_or(Constraint::Percentage(default_pct))
}

/// ±3 lines of context around the highlighted match's source line,
/// current line picked out in `theme.highlight`. Shows a muted
/// placeholder instead when there's no highlighted match or its source
/// pane text isn't available.
fn render_preview(frame: &mut Frame, area: Rect, state: &State) {
    let block = Block::default().borders(Borders::ALL).title("preview");
    let Some(m) = state.current_match() else {
        let p = Paragraph::new("no selection")
            .style(Style::default().fg(state.theme.muted))
            .block(block);
        frame.render_widget(p, area);
        return;
    };
    let Some(ctx) = preview::context_lines(m, &state.pane_texts) else {
        let p = Paragraph::new("no context available")
            .style(Style::default().fg(state.theme.muted))
            .block(block);
        frame.render_widget(p, area);
        return;
    };
    let lines: Vec<Line> = ctx
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i == ctx.current {
                let base = Style::default()
                    .fg(state.theme.highlight)
                    .add_modifier(Modifier::BOLD);
                let span_style = Style::default()
                    .fg(state.theme.accent)
                    .add_modifier(Modifier::BOLD);
                Line::from(split_at_span(line, ctx.span_chars, base, span_style))
            } else {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(state.theme.muted),
                ))
            }
        })
        .collect();
    let p = Paragraph::new(lines).block(block);
    frame.render_widget(p, area);
}

/// `grab:<name>` plus the resolved line cap, except for `viewport` -
/// its capture is the visible screen, not a line count, so a number
/// there would be misleading. `full` and any custom profile left
/// unbounded shows `(unbounded)` instead of a number.
fn grab_label_text(state: &State) -> String {
    let name = state.grab_cycler.current_name();
    let profile = state.grab_cycler.current_profile();
    if profile.source == crate::grab::GrabSource::Viewport {
        return format!("grab:{name}");
    }
    match profile.lines {
        Some(n) => format!("grab:{name} ({n})"),
        None => format!("grab:{name} (unbounded)"),
    }
}

fn render_grab_label(frame: &mut Frame, area: Rect, state: &State, label: &str) {
    let p = Paragraph::new(Line::from(Span::styled(
        label.to_string(),
        Style::default()
            .fg(state.theme.accent)
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(ratatui::layout::Alignment::Center)
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(p, area);
}

fn render_input(frame: &mut Frame, area: Rect, state: &State) {
    let mut spans = vec![
        Span::styled(
            "▍ ",
            Style::default()
                .fg(state.theme.cursor_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(state.query.clone()),
        Span::styled(
            "█",
            Style::default()
                .fg(state.theme.muted)
                .add_modifier(Modifier::DIM),
        ),
        Span::raw("   "),
    ];
    for inc in &state.parsed_query.includes {
        spans.push(Span::styled(
            format!("[{inc}]"),
            Style::default()
                .fg(state.theme.color_for_tag(inc))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }
    for exc in &state.parsed_query.excludes {
        spans.push(Span::styled(
            format!("[-{exc}]"),
            Style::default()
                .fg(state.theme.muted)
                .add_modifier(Modifier::DIM),
        ));
        spans.push(Span::raw(" "));
    }
    let count_text = if state.selected.is_empty() {
        format!("{}/{}", state.filtered.len(), state.matches.len())
    } else {
        format!(
            "{} sel * {}/{}",
            state.selected.len(),
            state.filtered.len(),
            state.matches.len()
        )
    };
    spans.push(Span::styled(
        count_text,
        Style::default().fg(state.theme.muted),
    ));
    spans.push(Span::raw("   "));
    let mode_tag = match state.mode {
        Mode::Input => "[INPUT]",
        Mode::List => "[LIST]",
    };
    spans.push(Span::styled(
        mode_tag,
        Style::default()
            .fg(state.theme.muted)
            .add_modifier(Modifier::DIM),
    ));
    let p = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("zextract  (Esc cancel, Tab mode, #type filter)"),
    );
    frame.render_widget(p, area);
}

fn render_list(frame: &mut Frame, area: Rect, state: &mut State) {
    if state.matches.is_empty() {
        let p = Paragraph::new("No matches in pane scrollback.")
            .style(Style::default().fg(state.theme.muted))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
        return;
    }
    if state.filtered.is_empty() {
        let p = Paragraph::new(format!("No matches for \"{}\"", state.query))
            .style(Style::default().fg(state.theme.muted))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
        return;
    }

    // Pane-title prefix only when more than one pane actually
    // contributed matches (tab-scan grab) - omitted entirely in
    // single-pane mode, matching the original's rule.
    let distinct_panes: HashSet<&str> = state
        .matches
        .iter()
        .filter_map(|m| m.source_pane_title())
        .collect();
    let show_pane_prefix = distinct_panes.len() > 1;

    let items: Vec<ListItem> = state
        .filtered
        .iter()
        .filter_map(|s| state.matches.get(s.index).map(|m| (s, m)))
        .map(|(s, m)| {
            // Leftmost gutter marks multi-selected rows; the `▸` cursor
            // (highlight_symbol below) sits between this and the tag,
            // so both signals coexist without colliding.
            let gutter = if state.selected.contains(&s.index) {
                Span::styled(
                    "* ",
                    Style::default()
                        .fg(state.theme.accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            };
            let pane_span = show_pane_prefix.then(|| {
                let title = truncate_display(m.source_pane_title().unwrap_or("?"), 15, false);
                Span::styled(
                    format!("[{title}]  "),
                    Style::default()
                        .fg(state.theme.muted)
                        .add_modifier(Modifier::DIM),
                )
            });
            let pane_overhead = pane_span.as_ref().map_or(0, |s| s.content.chars().count());
            let tag_span = Span::styled(
                format!("[{}]  ", m.effective_tag()),
                Style::default().fg(state.theme.color_for_type(m.ty)),
            );
            let tag_overhead = m.effective_tag().chars().count() + 8 + pane_overhead; // gutter(2) + "[tag]  "
            let avail = (area.width as usize).saturating_sub(tag_overhead);
            let use_middle = matches!(
                m.ty,
                MatchType::Url | MatchType::File | MatchType::Diagnostic | MatchType::Git
            );
            let display = truncate_display(&m.display, avail, use_middle);
            let mut spans = vec![gutter];
            if let Some(ps) = pane_span {
                spans.push(ps);
            }
            spans.push(tag_span);
            spans.extend(highlight_spans(&display, &s.indices, state.theme.highlight));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(state.theme.cursor_bg)
                .fg(state.theme.cursor_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, area, &mut state.list_state);
}

/// Bottom row: the config-missing banner takes priority (persistent
/// until `Ctrl-W`/`Ctrl-X`), then a transient status `message`, then
/// the default: a mode-aware footer of available verb-key hints for
/// the highlighted match. Ported from the original's
/// `render_banner`/`render_footer` split.
fn render_footer_or_banner(frame: &mut Frame, area: Rect, state: &State) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    if state.config_missing && !state.config_missing_dismissed {
        let lines = vec![
            Line::from(vec![
                Span::raw(" "),
                Span::styled("No config file found", bold),
                Span::raw("  -  defaults in use"),
            ]),
            Line::from(vec![
                Span::raw(" "),
                Span::styled("Ctrl-W", bold),
                Span::raw(": write default config    "),
                Span::styled("Ctrl-X", bold),
                Span::raw(": dismiss"),
            ]),
        ];
        let p = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.highlight)),
        );
        frame.render_widget(p, area);
        return;
    }
    if let Some((msg, is_error)) = &state.message {
        let color = if *is_error {
            state.theme.error
        } else {
            state.theme.highlight
        };
        let p = Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                msg.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
        return;
    }
    render_footer(frame, area, state);
}

fn render_footer(frame: &mut Frame, area: Rect, state: &State) {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(state.theme.muted);

    let mut line1: Vec<Span<'static>> = Vec::new();
    if let Some(m) = state.current_match() {
        let default = actions::default_verb(m, &state.config);
        line1.push(Span::styled(
            format!(" {}", m.effective_tag()),
            Style::default()
                .fg(state.theme.color_for_type(m.ty))
                .add_modifier(Modifier::BOLD),
        ));
        line1.push(Span::raw("  *  "));
        line1.push(Span::styled("Enter", bold));
        line1.push(Span::raw(format!(":{}  ", default.label())));

        if state.mode == Mode::List {
            for verb in actions::allowed_verbs(m, &state.config) {
                if verb == default {
                    continue; // already shown as Enter:label
                }
                line1.push(Span::styled(verb.key_label(), bold));
                line1.push(Span::raw(format!(":{}  ", verb.label())));
            }
            line1.push(Span::styled("J", bold));
            line1.push(Span::raw(":export  "));
            line1.push(Span::styled("Space", bold));
            line1.push(Span::raw(":select  "));
            line1.push(Span::styled("p", bold));
            line1.push(Span::raw(format!(
                ":{}preview  ",
                if state.preview_open { "hide " } else { "" }
            )));
        }
    } else {
        line1.push(Span::raw(" "));
        line1.push(Span::styled("no selection", dim));
    }

    // Universal-shortcut hints, shown in Input mode only - in List
    // mode the plain-letter equivalents are already on line 1, so
    // repeating the Ctrl-/Tab- forms would clutter without adding
    // info. The shortcuts still work in List mode, just hidden here.
    let mut line2: Vec<Span<'static>> = vec![Span::raw(" ")];
    if state.mode == Mode::Input {
        line2.push(Span::styled("Tab", bold));
        line2.push(Span::raw(":list mode    "));
        line2.push(Span::styled("Ctrl-Y", bold));
        line2.push(Span::raw(":copy    "));
        line2.push(Span::styled("Ctrl-A", bold));
        line2.push(Span::raw("/"));
        line2.push(Span::styled("Ctrl-D", bold));
        line2.push(Span::raw(":select all/none    "));
        line2.push(Span::styled("Ctrl-G", bold));
        line2.push(Span::raw(":next grabber    "));
        line2.push(Span::styled("Ctrl-P", bold));
        line2.push(Span::raw(":preview"));
    } else {
        line2.push(Span::styled(
            format!("{} selected", state.selected.len()),
            dim,
        ));
    }

    let p = Paragraph::new(vec![Line::from(line1), Line::from(line2)])
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(p, area);
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

/// Splits `line` into up to 3 spans around the char-index range
/// `span_chars`: `base` style everywhere, `span_style` for the range
/// itself. Used by [`render_preview`] to pick the exact extracted
/// finding out of its already-highlighted current line - an empty or
/// out-of-range `span_chars` (`start >= end`) just returns `line`
/// entirely in `base`.
fn split_at_span(
    line: &str,
    span_chars: (usize, usize),
    base: Style,
    span_style: Style,
) -> Vec<Span<'static>> {
    let (start, end) = span_chars;
    if start >= end {
        return vec![Span::styled(line.to_string(), base)];
    }
    let chars: Vec<char> = line.chars().collect();
    let start = start.min(chars.len());
    let end = end.min(chars.len());
    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::styled(
            chars[..start].iter().collect::<String>(),
            base,
        ));
    }
    if end > start {
        spans.push(Span::styled(
            chars[start..end].iter().collect::<String>(),
            span_style,
        ));
    }
    if end < chars.len() {
        spans.push(Span::styled(chars[end..].iter().collect::<String>(), base));
    }
    spans
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

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn parse_color_recognizes_ansi_names() {
        assert_eq!(parse_color("dark_gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("light_red"), Some(Color::LightRed));
    }

    #[test]
    fn parse_color_recognizes_hex() {
        assert_eq!(parse_color("#89b4fa"), Some(Color::Rgb(0x89, 0xb4, 0xfa)));
    }

    #[test]
    fn parse_color_recognizes_rgb() {
        assert_eq!(parse_color("rgb(10, 20, 30)"), Some(Color::Rgb(10, 20, 30)));
    }

    #[test]
    fn parse_color_rejects_unknown() {
        assert_eq!(parse_color("not-a-color"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        assert_eq!(parse_color("rgb(1,2)"), None);
    }

    #[test]
    fn theme_resolve_uses_builtin_defaults_with_zero_config() {
        let theme = Theme::resolve(&crate::config::ColorsConfig::default());
        assert_eq!(theme.muted, Color::DarkGray);
        assert_eq!(theme.color_for_type(MatchType::Url), Color::Blue);
    }

    #[test]
    fn theme_resolve_applies_override() {
        let colors = crate::config::ColorsConfig {
            cursor_bg: Some("#7aa2f7".to_string()),
            ..crate::config::ColorsConfig::default()
        };
        let theme = Theme::resolve(&colors);
        assert_eq!(theme.cursor_bg, Color::Rgb(0x7a, 0xa2, 0xf7));
    }

    #[test]
    fn theme_resolve_falls_back_on_unrecognized_override() {
        let colors = crate::config::ColorsConfig {
            highlight: Some("bogus".to_string()),
            ..crate::config::ColorsConfig::default()
        };
        let theme = Theme::resolve(&colors);
        assert_eq!(theme.highlight, Color::Yellow);
    }

    #[test]
    fn color_for_tag_falls_back_to_fallback_type_for_custom_pattern() {
        let theme = Theme::resolve(&crate::config::ColorsConfig::default());
        assert_eq!(theme.color_for_tag("jira"), Color::Gray);
    }
}

#[cfg(test)]
mod grab_label_tests {
    use super::*;

    fn state_at(initial_grab_name: &str) -> State {
        let config = crate::config::Config::default();
        let grab_cycler = crate::grab::GrabCycler::new(
            &config,
            &HashSet::new(),
            None,
            initial_grab_name,
            "/tmp/socket".to_string(),
            "pane1".to_string(),
            "tab1".to_string(),
        );
        State::new(LaunchArgs {
            matches: Vec::new(),
            pane_texts: HashMap::new(),
            custom_tags: &[],
            config_missing: false,
            config: &config,
            initial_query: "",
            preview_open: false,
            grab_cycler,
        })
    }

    #[test]
    fn shows_line_cap_for_quick() {
        assert_eq!(grab_label_text(&state_at("quick")), "grab:quick (150)");
    }

    #[test]
    fn shows_unbounded_for_full() {
        assert_eq!(grab_label_text(&state_at("full")), "grab:full (unbounded)");
    }

    #[test]
    fn omits_line_cap_for_viewport() {
        assert_eq!(grab_label_text(&state_at("viewport")), "grab:viewport");
    }
}

#[cfg(test)]
mod width_constraint_tests {
    use super::*;

    #[test]
    fn parses_percent() {
        assert_eq!(width_constraint("90%", 70), Constraint::Percentage(90));
    }

    #[test]
    fn parses_bare_cell_count() {
        assert_eq!(width_constraint("120", 70), Constraint::Length(120));
    }

    #[test]
    fn falls_back_on_out_of_range_percent() {
        assert_eq!(width_constraint("150%", 70), Constraint::Percentage(70));
    }

    #[test]
    fn falls_back_on_garbage() {
        assert_eq!(width_constraint("nonsense", 70), Constraint::Percentage(70));
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(width_constraint(" 90% ", 70), Constraint::Percentage(90));
    }
}

#[cfg(test)]
mod split_at_span_tests {
    use super::*;

    fn plain(text: &str) -> Span<'static> {
        Span::styled(text.to_string(), Style::default())
    }

    #[test]
    fn splits_around_a_middle_span() {
        let base = Style::default();
        let span_style = Style::default().add_modifier(Modifier::BOLD);
        let spans = split_at_span("see https://x.com end", (4, 17), base, span_style);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "see ");
        assert_eq!(spans[1].content, "https://x.com");
        assert_eq!(spans[1].style, span_style);
        assert_eq!(spans[2].content, " end");
    }

    #[test]
    fn span_at_the_very_start_has_no_leading_segment() {
        let spans = split_at_span("abc", (0, 1), Style::default(), Style::default());
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "a");
        assert_eq!(spans[1].content, "bc");
    }

    #[test]
    fn span_at_the_very_end_has_no_trailing_segment() {
        let spans = split_at_span("abc", (2, 3), Style::default(), Style::default());
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "ab");
        assert_eq!(spans[1].content, "c");
    }

    #[test]
    fn empty_span_returns_whole_line_unstyled_by_span_style() {
        let spans = split_at_span("abc", (0, 0), Style::default(), Style::default());
        assert_eq!(spans, vec![plain("abc")]);
    }

    #[test]
    fn out_of_range_span_is_treated_as_empty() {
        let spans = split_at_span("abc", (5, 2), Style::default(), Style::default());
        assert_eq!(spans, vec![plain("abc")]);
    }
}
