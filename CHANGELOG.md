# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-18

First release: a Herdr port of the `zellij-zextract` plugin, reaching
functional parity with the original (bar two documented platform gaps)
plus a few Herdr-native additions.

### Added

- **Socket client & grab.** Native `pane.read`/`pane.list` client
  against Herdr's socket API - no WASM, no async runtime. Grab
  profiles (`quick`/`deep`/`viewport`/`full`/`tab-scan`), fully
  overridable and user-extensible via `[grab_profiles.<name>]`.
- **Matcher engine.** URL, file, diagnostic, git, sha, ipv4, ipv6,
  uuid, quoted-string, command, and secret pattern types, plus
  user-defined `[[patterns.custom]]` regexes with template expansion.
- **Picker UI.** Fuzzy-filterable `ratatui` list with `#type`
  include/exclude query tokens, Input/List modes, multi-select
  (`Space`/`Ctrl-A`/`Ctrl-D`), a footer of context-aware verb hints,
  and a config-missing banner with a `Ctrl-W` "write starter config"
  affordance.
- **Actions.** copy/copy-display, insert/insert-display, open, edit,
  reveal, and JSON export, all with per-type allow-lists and defaults,
  per-verb dispatch caps, and user-overridable command templates
  (`[types.<tag>]`, `[actions.<tag>]`, `[limits]`).
- **Config.** `config.toml` (TOML, not the original's KDL) covering
  `log_level`, `[patterns]`, `[profiles.<name>]`, `[grab_profiles.<name>]`,
  `[ui]`, `[colors]` (full theme override, five built-in presets),
  `[types]`, `[actions]`, and `[limits]` - see
  `doc/config-reference.md`.
- **Manifest & keybinding.** `herdr-plugin.toml` with four ready-to-use
  actions (current pane / whole tab / URLs-only variants) plus ten free
  `customN` slots for user-defined profile combinations - no manifest
  edits needed to add a new keybind's behavior.
- **Grab-profile cycling.** `Ctrl-G` cycles the running picker through
  every configured grab profile, re-capturing and re-extracting in
  place, with the active grabber and its line cap shown next to the
  query field.
- **Preview pane.** `p`/`Ctrl-P` toggles a split showing ±3 lines of
  context around the highlighted match, with the match's own line and
  its exact extracted text both picked out in distinct colors.
- **CI & release.** `cargo fmt`/`clippy -D warnings`/`test` on every
  PR (macOS + Linux); tagged releases build and publish binaries for
  `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, and
  `aarch64-unknown-linux-gnu` (no Intel Mac binary - build from source
  there instead).

### Known platform gaps (Herdr has no equivalent yet)

- Per-launch popup title override (the original's `popupTitle`) - the
  popup's title is fixed by `[[panes]].title` at manifest-link time.
- Live popup resize when the preview toggles open - the preview is an
  internal split of the picker's own fixed render area instead (see
  `doc/config-reference.md`'s `[ui]` section for why).

[0.1.0]: https://github.com/codingfragments/herdr-zextract/releases/tag/v0.1.0
