//! The multiplexer host this binary runs under, behind one small trait.
//!
//! Everything else in the crate (matchers, picker, actions) talks to the
//! multiplexer only through [`Host`], so supporting another multiplexer
//! means adding one implementation here (see `rex-migration.md`).
//!
//! **Built to move into the shared `muxhost` crate later**
//! (rex-migration.md, "Built to move into `muxhost` later"). Keep this
//! module self-contained:
//! - Use only `std` and `serde_json`. Nothing from the rest of the crate:
//!   no `crate::config`, `crate::matcher`, `crate::actions`, and no
//!   zextract types in signatures.
//! - Errors are plain `String`s, and the module does not log.
//! - Env var names are generic. The tool prefix is passed to [`detect`]
//!   (`detect("ZEXTRACT")` reads `ZEXTRACT_HOST`, …).
//! - Enums are `#[non_exhaustive]` and optional capabilities get default
//!   methods, so later additions (colored reads for flash, scroll info)
//!   don't break existing implementations.

pub mod herdr;
mod socket_client;

#[cfg(test)]
pub mod fake;

/// Which part of a pane's buffer to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadSource {
    /// Only what the pane currently shows on screen.
    Viewport,
    /// The scrollback, soft-wrapped lines joined, newest line last.
    Scrollback,
}

/// The pane (and its tab) the tool was launched from: the one to grab
/// from by default and to insert text back into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub pane_id: String,
    /// Empty when the host could not tell (e.g. herdr's dev-testing
    /// fallback): tab-wide operations then find no panes.
    pub tab_id: String,
}

/// One pane of the source tab, as listed by [`Host::list_tab_panes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    pub pane_id: String,
    /// Human-readable title: the pane's label, else its terminal title,
    /// else a generated `pane <id>`.
    pub title: String,
}

/// The multiplexer operations the tool needs.
pub trait Host {
    /// Short host name, e.g. `"herdr"`.
    fn name(&self) -> &'static str;

    /// The pane the tool was launched from.
    fn source(&self) -> &SourceRef;

    /// Read `pane_id`'s text. `lines` caps the read to the newest N lines
    /// (`None` = everything the source holds).
    fn read_pane(
        &self,
        pane_id: &str,
        source: ReadSource,
        lines: Option<u32>,
    ) -> Result<String, String>;

    /// Every pane on the source tab, the source pane first, the rest in
    /// the host's own order. Floating/popup panes (including the tool's
    /// own) are left out.
    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String>;

    /// Type `text` into `pane_id` as if the user had typed it.
    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String>;
}

/// Pick the host from the environment.
///
/// `prefix` is the tool's env prefix (e.g. `"ZEXTRACT"`): an explicit
/// `<prefix>_HOST=herdr` wins; otherwise herdr is chosen when
/// `HERDR_SOCKET_PATH` is set. (Rex support follows in rex-migration.md
/// Phase 3.)
pub fn detect(prefix: &str) -> Result<Box<dyn Host>, String> {
    let host_var = format!("{prefix}_HOST");
    match std::env::var(&host_var).ok().as_deref() {
        Some("herdr") => return Ok(Box::new(herdr::HerdrHost::from_env()?)),
        Some(other) if !other.is_empty() => {
            return Err(format!(
                "{host_var}={other:?} is not a supported host (supported: herdr)"
            ));
        }
        _ => {}
    }
    if std::env::var_os("HERDR_SOCKET_PATH").is_some() {
        return Ok(Box::new(herdr::HerdrHost::from_env()?));
    }
    Err(format!(
        "no multiplexer host detected: HERDR_SOCKET_PATH is not set (or set {host_var}=herdr)"
    ))
}
