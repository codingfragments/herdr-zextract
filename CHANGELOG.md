# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - 2026-10-16

### Added

- A picker opened with **zero findings stays open** instead of
  closing immediately: the list shows the active grab profile and a
  `Ctrl-G` hint (`No matches under grab:quick (150) / Ctrl-G cycles
  grab profiles · Esc closes`), and the footer carries the same
  message on open — so cycling into another grab scope (or checking
  the query pre-fill) never requires relaunching. A failed capture
  (socket error) still exits.
- `[profiles.<name>].grab_cycle = ["deep", "full"]` — restrict which
  grab profiles `Ctrl-G` cycles through for that keybind. Listed
  order preserved, unknown names dropped, an all-unknown list falls
  back to the full cycle. `grab` picks where the cycle starts; a
  `grab` outside the list starts at its head.

## [0.5.0] - 2026-10-16

### Added

- Freshness display is now configurable per surface, replacing the
  fixed one-surface-at-a-time coupling from 0.4.1:
  - `[ui].freshness_row = "auto" | "on" | "off"` — the `↑N` row
    column. `auto` (default) = only while the preview split is
    closed; `on` = always, so you can compare distances across all
    rows *with the preview open* (its title shows only the
    highlighted row's position); `off` = never
  - `[ui].freshness_preview = "auto" | "on" | "off"` — the
    `preview · L1204/1309 · 105↑` border title; `off` gives a plain
    `preview` title
  - both default to `auto`, i.e. the 0.4.1 behavior

### Changed

- The single `[ui].freshness = "both" | "preview" | "row" | "off"`
  key from 0.4.0–0.4.1 is **removed** in favor of the two tri-states.
  Unknown keys are ignored by serde, so a stale config degrades to
  the defaults rather than breaking.

## [0.4.1] - 2026-10-16

### Fixed

- The freshness `↑N` row label moved to the *end* of each list row —
  between the `[tag]` pill and the match text it was too noisy — and
  now renders only while the preview split is *closed*: with the
  preview open its border title already carries the position, so the
  column was redundant. `[ui].freshness = "both"` now means exactly
  one display surface at a time (rows while closed, title while
  open).

## [0.4.0] - 2026-10-16

### Added

- Freshness: every match now carries its position signal — how far
  above the bottom of its pane's scroll buffer it sits (`↑105`,
  `↑0` = newest line). With equal fuzzy scores the list orders
  freshest-first; with an empty query every score is 0, so the plain
  opened list is fully freshness-ordered. This is what tells
  near-identical matches apart — the same command or URL printed
  repeatedly, differing only in a token.
  - `[ui].freshness = "both" | "preview" | "row" | "off"` (default
    `both`) controls the two displays: a dim `↑N` column after the
    `[tag]` pill on each list row, and the preview border title
    `preview · L1204/1309 · 105↑` (exact buffer line + distance from
    the bottom; blocks report their first content line)
  - the tie-break sort itself always applies; stale matches whose
    pane text is gone show `↑?` and sort last

## [0.3.0] - 2026-10-16

### Added

- New `block` match type — whole code-block extraction. The picker now
  also finds the snippet shapes modern codegen CLIs and READMEs show
  and offers each as a single row whose value is the **stripped**
  content (fence rows, frame rules, side bars, and number-gutter
  prefixes removed):
  - `fenced` — ` ``` `/`~~~` blocks from catted markdown; the info
    string survives as the row label and the `lang` field
    (`sh · 3 lines · curl …`)
  - `box` — box-framed sample blocks (pi/AI-CLI rounded frames, TUI
    boxes), full frames or clipped by the capture edge; boxed+numbered
    content strips both layers
  - `gutter` — `bat`/`cat -n`/`rg`-style line-numbered listings
  - filter with `#block` (`#!block` to hide), pin verbs like `cmd`
    (insert/copy-raw), `type_block` color slot, and a block-aware
    preview showing the whole block (24-line cap, `+N more lines`)
  - blocks over 200 lines are skipped, and the matches *inside* a
    block (URLs, commands, …) still appear as their own rows
  - box/gutter detection is ported from the sister `herdr-flash`
    plugin's `decor.rs`
- `just install` — swap the local dev link for the released plugin
  from GitHub: unlinks any dev plugin and runs `herdr plugin install
  codingfragments/herdr-zextract`, defaulting to the rolling `latest`
  tag; pin with `just install v0.3.0`.

## [0.2.0] - 2026-10-08

### Added

- `[patterns.command]` gains three keys for tuning `cmd` detection at
  runtime, no rebuild needed:
  - `triggers` — extra exec-anchored trigger executables, merged into
    the built-in list (never replacing it, so a partial list keeps
    `git`/`cargo`/`curl`/... working). For tools that aren't built in
    (e.g. `pi`, `terraform`).
  - `triggers_ignore` — words removed from the effective trigger list,
    for pruning a built-in that false-positives on scrollback (e.g. `go`
    matching the English word "go"). Wins over `triggers` on overlap;
    emptying the whole list disables exec-anchored detection entirely.
  - `prompt_markers` — extra prompt-anchored markers, merged into the
    built-ins and matched verbatim including trailing space, for
    custom shell prompts.
- `config.example.toml`: placeholder for `[profiles.<name>].type_filter`
  (query pre-fill) — previously the one documented key missing from
  the starter template.

## [0.1.1] - 2026-08-18

### Changed

- `url`/`url-tab` built-in profiles now default to `preview = "on"` -
  matches for these are always short (a URL/IP), so the preview's
  ±3-line context is cheap to show and usually worth it. `open`/`tab`
  are unchanged, staying on `[ui].preview`'s off default.

### Fixed

- Removed the trailing "-- press Enter to close --" prompt after an
  action - a Phase 1 debug leftover from before the picker existed.
  The popup now closes immediately once an action completes.

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

[0.1.1]: https://github.com/codingfragments/herdr-zextract/releases/tag/v0.1.1
[0.1.0]: https://github.com/codingfragments/herdr-zextract/releases/tag/v0.1.0
