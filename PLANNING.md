# Planning: herdr-zextract

## 1. Goal

Port [zextract](https://github.com/codingfragments/zellij-zextract)'s
functionality — scan focused-pane scrollback for typed patterns, present a
fuzzy-filterable picker, act on the match (open / edit / copy / insert /
export) — to run as a native [Herdr](https://herdr.dev) plugin, with the
same feature set and config surface as the original where practical.

Not a goal: feature growth beyond parity during the initial port. New
capabilities that only make sense on Herdr (e.g. multi-agent-aware actions)
are tracked separately in [§9 Ideas beyond parity](#9-ideas-beyond-parity),
not built into v1.

## 2. Original plugin reference

- Repo: https://github.com/codingfragments/zellij-zextract
- Host: Zellij, WASM plugin via the `zellij-tile` crate (ABI pinned to
  Zellij 0.44.x — see the original repo's `CLAUDE.md`)
- Architecture: workspace with `crates/zextract` (the plugin),
  `crates/spike-a-write-chars`, `crates/spike-b-ratatui` (exploration
  spikes for rendering approaches)
- Rendering: `ratatui`
- License: MIT

Docs worth re-reading before implementation: `doc/patterns.md` (built-in
regex patterns), `doc/config-reference.md` (user config schema),
`doc/customization.md`, `doc/use-cases.md`, `doc/types.md` (match-type →
action mapping).

## 3. Why Herdr is a different shape of host

Zellij plugins are WASM modules sandboxed behind the `zellij-tile` ABI —
no direct filesystem/network/process access; everything goes through
host-provided calls. Herdr plugins are **plain native processes** (any
argv command) that talk to a **local JSON socket API**
(`$HERDR_SOCKET_PATH`, newline-delimited JSON). A plugin pane declared with
`placement = "popup"` gets a real PTY — it's a normal terminal program, not
a sandboxed guest.

Practical consequence: the picker UI code (ratatui-based) needs almost no
change. The part that gets rewritten is everything that used to go through
`zellij-tile` calls — reading scrollback, sending text back to the source
pane, plugin registration/keybinding.

## 4. Architecture

```
┌─────────────────────────────────────────────┐
│ herdr-plugin.toml                            │
│  - [[panes]] entry: placement = "popup"      │
│    command = ["herdr-zextract"]              │
│  - [[keys.command]]: hotkey → plugin_action  │
└───────────────┬───────────────────────────────┘
                │ launches
                ▼
┌─────────────────────────────────────────────┐
│ herdr-zextract binary (Rust)                 │
│                                               │
│  1. socket_client.rs                         │
│     - connect $HERDR_SOCKET_PATH             │
│     - pane.read (source pane, recent-        │
│       unwrapped) → scrollback text           │
│     - pane.send_input / send_text            │
│       (insert action)                        │
│                                               │
│  2. matcher engine (ported ~as-is)           │
│     - built-in regex patterns                │
│     - user-defined patterns (config file)    │
│     - match → type → available actions       │
│                                               │
│  3. picker UI (ratatui, ported ~as-is)       │
│     - fuzzy filter over matches              │
│     - type-aware action menu                 │
│                                               │
│  4. actions.rs                               │
│     - open URL   → `open`/`xdg-open`         │
│     - edit file  → $EDITOR                   │
│     - copy       → arboard clipboard         │
│     - insert     → pane.send_input           │
│     - export     → write JSON to stdout/file │
└───────────────────────────────────────────────┘
```

The plugin context (`HERDR_PLUGIN_CONTEXT_JSON`) carries the originating
pane/tab/workspace id at launch time as `focused_pane_id` — confirmed
against a real Herdr install, see [§12 Open questions](#12-open-questions).

## 5. Migration map: zellij-tile → herdr socket API

| Original (`zellij-tile`) | Herdr equivalent |
|---|---|
| Plugin registration, `register_plugin!` | `herdr-plugin.toml` manifest, `[[panes]]` entry |
| Keybind → `LaunchOrFocusPlugin` | `[[keys.command]]` → `plugin_action` |
| Read focused pane content (host-provided buffer) | `pane.read` (`source = "recent-unwrapped"`) over the socket |
| Write/paste into pane | `pane.send_input` / `pane.send_text` |
| Floating pane sizing (`size` config key) | Popup `width`/`height` in `[[panes]]` (cells or %) |
| Plugin config (KDL block in Zellij config) | Plugin's own config file under `$HERDR_PLUGIN_CONFIG_DIR` |
| Clipboard (host `Clipboard` action) | `arboard` crate directly (plugin has full OS access) |
| Open URL/editor (host action, if any) | Shell out via `std::process::Command` |

Confirmed in Phase 1 (see `phase/1-socket-client-echo`): every socket
response payload is nested one level deeper than its JSON Schema
`$defs` entry suggests — e.g. `pane.read`'s result is
`{"type":"pane_read","read":{...PaneReadResult fields...}}`, not the
`PaneReadResult` fields directly at the top of `result`. Same pattern
for `plugin.pane.open` → `{"type":"plugin_pane_opened","plugin_pane":
{...}}`. Expect this `{"type": "...", "<name>": payload}` envelope for
every socket call in later phases (actions' `pane.send_input`, etc.).

## 6. Language & dependency choice

**Rust**, carrying over as much of the original crate's logic as possible.

Rationale:
- The matcher engine, fuzzy filter, and `ratatui` picker UI already exist
  in Rust in the original repo and are host-agnostic — they don't touch
  `zellij-tile` directly, so they should move over close to unchanged.
- Herdr plugins are no longer WASM-constrained, so the build actually gets
  *simpler* than the original (`wasm32-wasip1` target dropped entirely;
  native `cargo build --release` per platform).
- Cross-platform crates needed either way:
  - `ratatui` + `crossterm` — terminal UI, already macOS/Linux-portable.
  - `serde` / `serde_json` — socket protocol (newline-delimited JSON).
  - `arboard` — clipboard, covers macOS and Linux (X11 + Wayland) without
    hand-rolling `pbcopy`/`xclip`/`wl-copy` branches.
  - `regex` — pattern matching (same as original).
  - A small hand-rolled Unix-socket client (`std::os::unix::net::UnixStream`)
    — no need for a full async runtime for this workload; a blocking
    request/response client is enough for a foreground picker.

## 7. Portability plan (macOS + Linux)

- Target triples: `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
  `aarch64-unknown-linux-gnu`. No `x86_64-apple-darwin` (Intel Mac) build
  - not worth the CI minutes for a platform with negligible remaining
  install base among this plugin's users.
- No Windows target for now (not a requirement; Herdr's Windows beta uses
  a different IPC transport — named pipes — which this plugin won't
  implement initially).
- Clipboard and "open URL" both need OS-specific fallback logic even with
  `arboard`/`open`-style crates — keep this isolated in `actions.rs` behind
  a small trait so platform quirks don't leak into the matcher/UI code.
- Avoid glibc-specific behavior on Linux where feasible; evaluate
  `cross`/`musl` static linking for the Linux binaries to reduce
  target-machine dependency issues (glibc version skew) — decide during
  CI setup, not blocking for planning.

## 8. Install & distribution plan

Two supported install paths, both anchored on tagged GitHub releases:

**A. Source install via `herdr plugin install`**
```sh
herdr plugin install codingfragments/herdr-zextract
```
**Important — this is not automatic build detection.** Herdr does not
inspect a cloned repo and decide "this is a Rust project, run `cargo`."
It only runs a build step if the manifest explicitly declares one via
`[[build]]`, e.g.:
```toml
[[build]]
command = ["cargo", "build", "--release"]
```
`herdr plugin install` clones the repo, runs whatever `[[build]]` commands
are declared (in order, before registration), and registers the plugin.
If the manifest has no `[[build]]` section, install just clones and
registers without compiling anything — so the manifest for this repo
**must** carry the `cargo build --release` step above, and its `command`
must point at the resulting release binary path
(`target/release/herdr-zextract`), not a bare `herdr-zextract` (that only
resolves via `PATH`, which is Option B below).

This path requires a working Rust toolchain (`cargo`) on the machine
running `herdr plugin install` — Herdr "reports build failures but does
not install missing toolchains." Works for any ref, not just tagged
releases — good for tracking `main`.

Note: `herdr plugin link` (local-dev install of a directory you already
have checked out) skips `[[build]]` entirely regardless of manifest
contents — you're expected to `cargo build` your own working tree
yourself before linking.

**B. Binary install via `cargo install` from a labeled stable release**
```sh
cargo install --git https://github.com/codingfragments/herdr-zextract --tag v0.1.0 herdr-zextract
```
Installs the binary onto `PATH` directly from a tagged commit — no local
clone management needed. The `herdr-plugin.toml` for this path is minimal:
```toml
[[panes]]
id = "zextract"
placement = "popup"
command = ["herdr-zextract"]   # resolved via PATH, not a repo-relative path
```
This is the path recommended for most users once releases are stable —
it sidesteps the manifest's git-based `herdr plugin install` clone/build
step entirely and just requires the binary to already be on `PATH`.

**C. (stretch) Prebuilt binary download**
GitHub Actions attaches prebuilt binaries per target triple to each tagged
release (see CI plan below). A future `install.sh` (mirroring Herdr's own
`curl | sh` installer) could fetch the right binary directly without
`cargo install` compiling anything — nice-to-have once release cadence is
established, not required for v1.

### New-machine bootstrap (planned)

```sh
# 1. Ensure herdr itself is installed (https://herdr.dev/install.sh)
# 2. Install the plugin binary (path B, recommended once released)
cargo install --git https://github.com/codingfragments/herdr-zextract --tag v0.1.0
# 3. Register the plugin with herdr (path/manifest TBD once manifest is written)
herdr plugin link ~/.cargo/... # or point manifest command at the installed binary
# 4. Bind a key in herdr config to the plugin action
```
Exact `herdr plugin install`/`link` invocation to be finalized once the
manifest is written and tested against a real Herdr install.

## 9. CI / release plan (GitHub Actions)

Implemented in `.github/workflows/release.yml` and `ci.yml` (Phase 10):

- Trigger: tag push matching `v*.*.*`.
- Matrix (three target triples, not four - `x86_64-apple-darwin` was
  dropped before the first release; not worth the CI minutes for a
  platform with negligible remaining install base among this plugin's
  users):
  - `macos-14` (arm64, native) → `aarch64-apple-darwin`
  - `ubuntu-latest` → `x86_64-unknown-linux-gnu`
  - `ubuntu-24.04-arm` (native ARM, GA in both public and private repos as
    of 2026-01-29) → `aarch64-unknown-linux-gnu` — no `cross`/QEMU needed,
    see [§12 Open questions](#12-open-questions)
- Steps per job: checkout → `cargo build --release --target <triple>` →
  strip binary → `sha256sum` → upload as release asset named
  `herdr-zextract-<triple>.tar.gz` (+ `.sha256`).
- Separate `ci.yml` for PRs/pushes to `main`: `cargo fmt --check`,
  `cargo clippy -- -D warnings`, `cargo test`, on macOS + Linux runners —
  mirrors the original repo's pre-push hook checks
  (`scripts/install-hooks.sh` pattern reused if useful).
- Tag naming: `vMAJOR.MINOR.PATCH`, first stable tag once picker + matcher
  parity with the original is reached and manually verified against a
  real Herdr session on both a Mac and a Linux box.
- Rolling `latest` tag: every release run also force-moves a `latest`
  git tag to the newly-tagged commit and publishes/updates a `latest`
  GitHub release with the same three binaries (`make_latest: false`, so
  it never displaces the real semver tag's "Latest release" badge) -
  lets `herdr plugin install <owner>/<repo> --ref latest` track newest
  without pinning a version. Introduced after v0.1.1.

## 10. Repo layout (planned, once code starts)

```
herdr-zextract/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── socket_client.rs   # herdr socket API client
│   ├── matcher/           # ported pattern-matching engine
│   ├── picker/            # ported ratatui picker UI
│   └── actions.rs         # open/edit/copy/insert/export, OS-specific bits
├── doc/
│   ├── patterns.md        # ported from original, updated for herdr paths
│   ├── config-reference.md
│   └── use-cases.md
├── herdr-plugin.toml       # manifest (written once binary exists)
├── .github/workflows/
│   ├── ci.yml
│   └── release.yml
├── PLANNING.md             # this file
└── README.md
```

## 11. Implementation phases

Superseded the original horizontal milestone list (matcher-then-UI-then-
host-wiring) once §12 unblocked real Herdr testing. Each phase below is a
**vertical slice**: it runs as a real popup inside a real Herdr session
and produces an observable result end-to-end, even though most of the
feature set is still stubbed. Each phase is one `phase/<slug>` branch/PR
per the gitflow in `CLAUDE.md`, merged before the next phase starts.

When starting a phase, open it with the "Prompt" text below verbatim (it
carries the goal and scope so a fresh session doesn't need the rest of
this document to get oriented) and don't consider the phase done until
its manual test plan passes against a real Herdr install.

### Phase 1 — Socket client + raw popup echo

**Prompt:** Build the smallest possible `herdr-zextract` binary and
manifest that proves the popup-and-socket plumbing works end-to-end: on
launch, read `HERDR_PLUGIN_CONTEXT_JSON` for `focused_pane_id`, call
`pane.read` over `$HERDR_SOCKET_PATH` with `source = recent_unwrapped`,
and print the raw scrollback text into the popup pane (no matching, no
picker, no ratatui yet — just prove the pipes connect). Write
`herdr-plugin.toml` with a `[[panes]]` popup entry and a `[[build]]`
step (`cargo build --release`), per §8. Keep the socket client
hand-rolled (`UnixStream`, newline-delimited JSON) per §6 — no async
runtime.

**Scope:** `socket_client.rs` (connect, one request/response round trip),
`main.rs` reading context env + printing result, minimal manifest.

**Out of scope:** matcher, picker UI, actions, config file, keybinding
(`herdr plugin pane open` from the CLI is an acceptable trigger for this
phase — real keybinding wiring is Phase 7).

**Manual test plan:**
1. `cargo build --release`.
2. `herdr plugin link .` from the repo root.
3. Focus any pane with some scrollback text, then trigger the plugin
   pane (`herdr plugin pane open --plugin <id> --entrypoint <id>
   --placement popup`).
4. Confirm the popup shows the scrollback of the pane that was focused
   *before* the popup opened, not the popup's own (empty) buffer.
5. `herdr plugin unlink <id>` when done.

### Phase 2 — Matcher engine, wired to real scrollback

**Prompt:** Port the original plugin's pattern-matching engine
(`doc/patterns.md`'s built-in regex set) into `matcher/`, with unit
tests that run standalone (no Herdr needed). Then wire it into the
Phase 1 binary: instead of dumping raw scrollback, run the matcher over
it and print the resulting match list (type + matched text + line) as
plain text in the popup.

**Scope:** `matcher/` module + unit tests, `main.rs` updated to run
matches over the `pane.read` result and print them.

**Out of scope:** picker UI (still plain text output), actions,
user-defined/custom patterns (built-ins only for now).

**Manual test plan:**
1. `cargo test` — matcher unit tests pass standalone.
2. In a real pane, produce scrollback with a URL, a file path, and a
   UUID (e.g. `echo https://example.com ./src/main.rs
   $(uuidgen)`).
3. Trigger the popup as in Phase 1's test plan.
4. Confirm the popup lists all three matches with correct type labels,
   and ignores unrelated scrollback lines.

**Done (verified 2026-08-17):** ported directly from the original repo's
`crates/zextract/src/{extract.rs,pattern/*.rs}` (fetched via `gh api`) —
url/uuid/ipv4/ipv6/quoted/sha/diagnostic/file/git/secret ported near
verbatim including their unit tests (94 pass standalone), plus `extract`'s
two-pass dedup (`dedup_keep_latest`, `dedup_by_raw_priority`) and
`TYPE_PRIORITY`. Two deliberate scope cuts vs. the original, both safe
because they're off-by-default there too:
  - `command`'s opt-in flag/comment/extension-anchored passes were not
    ported — only the default-on prompt-anchored + exec-anchored core.
  - Per-pattern `ExtractionTimings` and `PatternsConfig` (disabled-type
    set, custom patterns, command/secret tuning) deferred to Phase 5 —
    `secret`'s `entropy_filter` and `command`'s `rprompt_min_spaces` are
    hardcoded to their upstream defaults (`true`, `5`) for now.
Verified live: real scrollback containing a URL, a file:line:col, a
UUID, a bare SHA, and an AWS key all round-tripped through
`pane.read` → `matcher::extract` → popup correctly.

### Phase 3 — Picker UI

**Prompt:** Wire the Phase 2 match list into the original plugin's
`ratatui` picker UI (ported ~as-is per §4/§6 — it's host-agnostic). The
picker should support fuzzy-filtering the match list interactively.
Selecting a match should, for now, just print the selected match to
the popup and exit — no action execution yet (that's Phase 4).

**Scope:** `picker/` module (ported ratatui/crossterm UI), `main.rs`
driving it off the Phase 2 matcher output.

**Out of scope:** actions (open/edit/copy/insert/export), type-aware
action menu — selection just echoes the pick for now.

**Manual test plan:**
1. Trigger the popup against scrollback with several distinct match
   types.
2. Type a fuzzy filter substring and confirm the list narrows correctly.
3. Navigate with arrow keys, press Enter on a match, confirm the popup
   prints that exact match and closes cleanly (no hang, no zombie
   process — check with `ps` that the plugin process exits).
4. Press Esc/Ctrl-C and confirm the picker closes without error.

**Done (verified 2026-08-17):** ported `fuzzy.rs` (nucleo-matcher
wrapper) and `query.rs` (`#type`/`#!type` filter-token parsing) from
the original repo near-verbatim into `picker/` — both are pure and
host-agnostic. `picker/mod.rs` ports the original `State::refilter`,
`current_match`, `truncate_display`, and `highlight_spans` logic, but
**does not** port `render.rs`'s hand-rolled Buffer→ANSI emitter — that
existed only because Zellij's WASM host couldn't link `crossterm`
directly; Herdr plugins are native, so this uses `ratatui`'s standard
`CrosstermBackend` + a normal `Terminal::draw` event loop instead
(simpler, gets resize/cursor handling for free). Single input+list
layout, no Input/List mode split — with actions deferred to Phase 4
there are no list-mode verbs yet, so typing always edits the query.

Bug found & fixed during manual testing: `ratatui`'s diff renderer
assumes it starts from a blank terminal; without an explicit
`terminal.clear()` before the first draw, cells that happen to render
as blank in the first frame aren't force-written, leaving the pane's
prior scrollback visible through gaps in the picker UI. Fixed with one
`terminal.clear()` call right after `Terminal::new`.

Tested by running the binary inside a normal (non-popup, addressable)
pane with `HERDR_ACTIVE_PANE_ID` set to itself, then driving it via
`herdr pane send-keys` + reading frames back with `herdr pane read
--source visible` — popup panes aren't independently addressable (per
§12), so this was the only way to script full keyboard-interaction
coverage: typed-filter narrowing, `#sha` type-filter pill, arrow-key
navigation, Enter-to-select printing the exact match, Esc-to-cancel,
and clean process exit in both cases.

### Phase 4 — Actions

**Prompt:** Replace Phase 3's "print selection" with the real
type-aware action menu and action execution: **open** (URL via
`open`/`xdg-open`), **edit** (file via `$EDITOR`, at the matched line if
available), **copy** (`arboard` clipboard), **insert** (`pane.send_input`
back into the *source* pane captured from `focused_pane_id`), **export**
(write the current match set as JSON to stdout or a file). Keep
platform-specific bits behind a small trait in `actions.rs` per §7 so
matcher/UI stay platform-agnostic.

**Scope:** `actions.rs`, action-menu UI in `picker/`, `pane.send_input`
usage in `socket_client.rs`.

**Out of scope:** user config for action bindings, custom patterns —
still built-in patterns and a fixed action set per match type
(`doc/types.md`'s original mapping).

**Manual test plan:** for each action, trigger the popup, pick a
matching match type, and verify the concrete effect:
1. **open** — a URL match opens in the default browser.
2. **edit** — a file-path match (with a line number, e.g. a diagnostic)
   opens `$EDITOR` at that file/line.
3. **copy** — after picking copy, `pbpaste` (macOS) / clipboard tool
   (Linux) shows the matched text.
4. **insert** — after picking insert, the source pane (the one that was
   focused before the popup opened) receives the matched text as input.
5. **export** — the exported JSON file/stdout contains the expected
   match objects (type, text, line if applicable).

**Done (verified 2026-08-17):** ported `action.rs`'s allow-list/default
tables and dispatch logic into `actions.rs`. Enter fires the type's
default verb; `Ctrl-y`/`Ctrl-i`/`Ctrl-o`/`Ctrl-e`/`Ctrl-j` fire
copy/insert/open/edit/export explicitly (only when allowed for that
match's type — otherwise a silent no-op, picker stays open). Verified
against the doc, not just the doc's prose: `doc/types.md` claims `file`
defaults to `edit`, but the actual `static_default_verb` code puts it
in the `Insert` bucket with everything except `Url`/`Diagnostic` —
ported the code's behavior.

One deliberate departure from the original: its `edit` verb couldn't
spawn an interactive `$EDITOR` (no real TTY in a WASM plugin), so it
typed the editor command into the source pane for the user to run
themselves. Herdr plugins are native processes with a real PTY, so
`edit` here spawns `$EDITOR` directly — a genuine capability upgrade.

Two bugs found and fixed via manual testing:
- **Stale socket reuse**: `insert` was written to reuse the `pane.read`
  connection opened at process start, but by the time a human types a
  filter and picks a match, that connection can go stale server-side
  ("Broken pipe"). Fixed by opening a fresh connection right at insert
  time instead of holding one open across the interactive session.
- **Diagnostic regex ate leading slashes**: ported verbatim from the
  original, `diagnostic.rs`'s colon-form regex anchored on `\b`, which
  can't match immediately before a bare `/` (both sides non-word) — so
  `/tmp/foo.rs:2:1` matched as `tmp/foo.rs:2:1`, silently dropping the
  leading slash and breaking `edit` against absolute paths. Fixed by
  dropping the `\b` and checking the preceding byte in code instead,
  the same way `file.rs` already does it.

Tested each action against real effects: `open` actually launched the
browser; `insert` landed the exact text on the real *source* pane's
prompt (verified with two distinct panes — binary running in one,
scrollback/insert-target in another — since a popup can't be both);
`copy` verified via `pbpaste`; `export` produced valid JSON with all
expected fields; `edit` verified via a fake `$EDITOR` script logging
its argv (`+<line> <file>`); and the secret hardcoded-deny verified by
confirming `Ctrl-o` on a secret is silently ignored (no browser launch,
no crash).

### Phase 5 — User config & custom patterns

**Prompt:** Port `doc/config-reference.md`'s config schema: a config
file under `$HERDR_PLUGIN_CONFIG_DIR` that lets users add custom regex
patterns and adjust built-in pattern behavior, loaded at plugin startup
alongside the built-ins from Phase 2.

**Scope:** config file parsing/schema, merge logic with built-in
patterns, `doc/config-reference.md` ported/updated for Herdr paths.

**Out of scope:** live config reload while the popup is open (config is
read once per invocation, matching the original's snapshot-once model
per §13).

**Manual test plan:**
1. Add a custom pattern to the config file under
   `$(herdr plugin config-dir <id>)`.
2. Produce scrollback matching only the custom pattern.
3. Trigger the popup and confirm the custom match appears with the
   configured type/action set.
4. Remove/break the config file and confirm the plugin still runs
   correctly against just the built-ins (no crash on missing config).

**Done (verified 2026-08-17):** ported the `patterns { }` block's
scope — global `disable` list, `secret.entropy_filter` toggle, and
`[[patterns.custom]]` regex/type/template patterns — into `config.rs` +
`matcher/custom.rs`, loaded once from
`$HERDR_PLUGIN_CONFIG_DIR/config.toml`. One deliberate format change:
**TOML instead of KDL**, matching Herdr's own config conventions
(`config.toml`, `herdr-plugin.toml`) rather than Zellij's. Out of
scope, unchanged from the prompt: `ui`/`colors`/`grab`/`limits`/
`types`/`actions` blocks — those configure surfaces (theming, depth
profiles, per-verb caps, action templates) this port hasn't built.

`Match` didn't grow a dedicated `label` field for custom-pattern names
(would've meant touching all ten built-in pattern modules' `Match`
literals for a field only `custom.rs` ever sets) — instead
`custom::extract` stashes the name under `fields["__label"]`, and a new
`Match::effective_tag()` reads that when present. Picker and export
JSON both switched from `m.ty.tag()` to `m.effective_tag()` so a custom
pattern is filterable/exported by its own configured name.

Ported the two remaining upstream fixture tests deferred from Phase 3
(`multi_group_patterns.txt`, `custom_patterns.txt`) now that custom
patterns exist to test against - 141 tests total.

Verified live against a real Herdr install: wrote a `config.toml` with
`disable = ["ipv6"]` plus a `jira` custom pattern
(`([A-Z]+)-([0-9]+)` → `https://jira.example.com/browse/{1}-{2}`).
Confirmed `#jira` resolves as its own filter tag, the exported JSON
carries `"type":"jira"` and the expanded URL as `raw`, `ipv4` still
matches while `ipv6` doesn't, and — separately — that both a missing
config file and a malformed TOML file fall back to built-in defaults
without crashing.

**Follow-up:** added a `Ctrl-W` "write starter config" affordance,
matching the original's `Ctrl-W`-on-missing-config banner action. When
`$HERDR_PLUGIN_CONFIG_DIR/config.toml` doesn't exist, the input bar
shows a `(Ctrl-W: write starter config)` hint; pressing it writes a
heavily-commented starter file (`config::DEFAULT_CONFIG_TOML`) and
shows a status message. Only offered when the file is missing, not
when it exists-but-fails-to-parse (a broken config is the user's, not
ours to silently replace), and refuses to overwrite
(`OpenOptions::create_new`) as a defensive double-check. Verified live:
the hint appears, `Ctrl-W` writes a valid, immediately-loadable
`config.toml` and the hint disappears, and pressing it again after
that is a no-op.

Also clarified for the record: `$HERDR_PLUGIN_CONFIG_DIR` already
resolves under `~/.config/herdr` (specifically
`~/.config/herdr/plugins/config/herdr-zextract/`) — it's Herdr's own
per-plugin namespacing *within* the main herdr config dir, not a
separate location. Considered and rejected moving to a flat
`~/.config/herdr/zextract.toml`: that would bypass the official
mechanism and risk colliding with Herdr's own `config.toml` or other
plugins'.

### Gap analysis vs. the original (2026-08-17)

Phases 1-5 shipped a working, config-driven picker, but a careful
re-read of the original `zellij-zextract` source (`action.rs`,
`main.rs`'s `State`/`render_footer`/`render_banner`,
`doc/config-reference.md`, `doc/multi-pane-grab.md`, its README)
surfaced real functional gaps beyond what each phase's own "out of
scope" notes called out. Resolved via an interview with the user;
Phases 6-9 below exist because of it. Two items were explicitly put on
the **backburner** (not phases - implement ad hoc only if actually
needed, not preemptively):
- **Progress bar / incremental-extraction UI**: the original's
  chunked extraction + spinner + `LineGauge` existed mainly for
  Zellij's WASM per-frame render budget; our native process extracts
  synchronously and it's fast even on large text. Multi-pane scanning
  (Phase 7) could add up over many panes' `pane.read` round-trips, but
  don't build feedback UI for a slowness that hasn't been observed -
  revisit only if it's actually sluggish in practice.
- **`mask_secrets`** (replace secret values with `••••` in the list):
  cosmetic only - the secret is already sitting in plaintext in the
  scrollback it was extracted from, so masking it in the picker adds
  no real security. Not worth building.

### Phase 6 — Action menu, mode split, multi-select, banners

**Prompt:** Close the "I can't see what I can do with a match" gap.
Port the original's `render_footer` (dynamic verb-key hints for the
highlighted match's type) and the Input/List mode split (`Tab`
toggles; in List mode bare letters `y`/`i`/`o`/`e`/`r`/`Y`/`I`/`p`/`J`/
`Space` fire verbs directly instead of editing the query - the
Ctrl-modifier bindings from Phase 4 keep working as universal
shortcuts in both modes, matching the original's "Ctrl-Y works from
either mode" pattern). Add multi-select (`Space` toggles the
highlighted row; `Ctrl-A`/`Ctrl-D` select-all-visible/clear) with
batch dispatch (copy joins with `\n`, insert joins with a space, edit
chains with `&&`, open fires N separate invocations), gated by
hardcoded per-verb caps matching the original's defaults (copy 100,
insert 5, open 10, edit 5, json 100 - no `reveal` cap needed until
Phase 8 makes these real config). Add a dedicated banner row (replacing
the cramped inline-message hack from Phase 5) for the config-missing/
parse-error/warning states, with `Ctrl-X` to dismiss. Add the `reveal`
verb (open Finder/file-manager at the file - macOS `open -R`, check
what's idiomatic on Linux) and wire `Y`/`I` (CopyDisplay/InsertDisplay)
now that List mode exists to put them on.

**Scope:** `picker/mod.rs` (mode enum, footer/banner rendering,
multi-select state, batch dispatch), `actions.rs` (`reveal` verb, cap
table).

**Out of scope:** making the caps configurable (`limits{}` - Phase 8),
`colors{}`-driven banner/footer styling (Phase 8 also owns theming).

**Manual test plan:**
1. Highlight matches of several different types; confirm the footer
   shows that type's actual allowed verbs, not a static hint.
2. Press `Tab`, confirm typing no longer edits the query and bare
   letters fire verbs (`y` copies, `o` opens a URL match, etc.);
   `Tab` back to Input mode and confirm typing works again.
3. `Space` two rows, fire a batch copy, confirm the clipboard holds
   both values newline-joined. Try exceeding a verb's cap and confirm
   it's refused with a message, not a partial/silent fire.
4. Trigger the config-missing state and confirm the banner (not the
   input bar) shows it; `Ctrl-W` writes the config, `Ctrl-X` dismisses
   a banner without acting on it.
5. `r` on a file match opens it in Finder/file-manager; `Y`/`I` on a
   `quote` match copy/insert the unquoted `display` value.

**Done (verified 2026-08-17):** ported `render_footer`/`render_banner`
and the Input/List mode split into `picker/mod.rs`; ported
`allowed_verbs`/`static_default_verb`'s multi-target rules
(`plan_batch`/`execute_batch` in `actions.rs`) with hardcoded caps
matching the original's `limits{}` defaults (copy/json 100,
insert/edit 5, open/reveal 10). Added `reveal` (macOS `open -R`; Linux
falls back to `xdg-open` on the parent directory - no universal
"select in file manager" API exists there) and `Y`/`I`
(CopyDisplay/InsertDisplay), reachable now that List mode exists.

Real bug found via manual testing: `Ctrl-I` is byte-identical to `Tab`
(both `0x09`) in terminal protocols, so binding it to force-Insert
silently toggled the mode instead of firing anything. Fixed by
dropping `Ctrl-I` entirely and using `Shift-Enter` for force-insert
instead, matching what the original actually does (it was never
`Ctrl-I` there either - that binding was this port's own Phase-4-era
placeholder, invented before List mode existed to give every verb a
Ctrl-shortcut; once bare `i` covers List mode, `Ctrl-I` was both
redundant and broken).

Verified live against a real Herdr install (two-pane setup: binary
running in one pane, scrollback/insert-target in another, same
technique as Phase 3): footer shows the correct type-specific verb
hints and updates on `Tab`; bare `y` in List mode fires copy directly
(confirmed via `pbpaste`); `Space` + `Space` selects two rows (gutter
`*` marker, "N selected" count); batch copy joins with `\n`
(`pbpaste`-verified); selecting 8 matches and firing insert (cap 5)
shows "refused: 8 matches exceeds cap of 5 for 'insert'" and leaves the
selection intact rather than firing partially; the config-missing
banner appears in the dedicated footer/banner row (not squeezed into
the input bar), `Ctrl-W` writes the config, `Ctrl-X` dismisses the
banner without writing anything (confirmed by checking the config
directory stayed empty after dismiss); batch edit on two file matches
chains two invocations of a fake `$EDITOR` script via `&&`, each with
its own correct `+line` argument, verified via the fake editor's
logged argv. Shift-Enter's specific key combo couldn't be
independently exercised through the `herdr pane send-keys` CLI test
harness (it appears to have no effect at all via that path - likely a
harness/terminal-protocol limitation, not a confirmed code issue),
but the `try_fire`/`plan_batch` logic it shares is the exact path
already proven correct via plain `Enter` with a full 8-match selection.

### Phase 7 — Manifest polish & keybinding

**Prompt:** Finalize `herdr-plugin.toml` for both install paths in §8
(Option A's `[[build]]`-driven install, Option B's `cargo install`
minimal manifest). Port `grab{}` config (named scrollback-depth
profiles - `quick`/`deep`/`viewport`/`full`/`tab-scan`, runtime cycling
via `g`/`Alt-g`) and multi-pane tab-wide scanning (`source = "tab"`:
every non-floating, non-plugin pane on the active tab, last-focused
pane's matches first, pane-title-prefixed rows when more than one pane
contributes, insert always targets the pane the plugin was launched
from regardless of which pane a match came from - see
`doc/multi-pane-grab.md`'s design-decisions table, ported verbatim).
Wire real Herdr keybinds (`[[actions]]` in the manifest, per the schema
found in Phase 1's investigation) with **per-keybind configuration
overrides** - the manifest-level equivalent of the original's
`LaunchOrFocusPlugin` `configuration` map (`type` pre-fill filter,
`grab` profile override, `patterns` allowlist, `preview` state,
`popupTitle`) - since this is the actual mechanism the user's
`prefix-s`/`S`/`u`/`U` keybind setup (single-pane vs. cross-tab grab,
URL/IP-only quick filter) depends on. One keybind with no
per-bind configurability isn't the feature being asked for.

**Scope:** `grab.rs` or similar (profile config + multi-pane
extraction), manifest `[[actions]]` overrides, README's "Option A/B"
install instructions validated against both paths.

**Out of scope:** CI/release automation (Phase 10), `colors{}`/`ui{}`/
`types{}`/`actions{}` config (Phase 8), preview pane (Phase 9).

**Process rule:** any edit to the user's live
`~/.config/herdr/config.toml` (adding/changing `[[keys.command]]` or
plugin keybind entries) is proposed as an explicit diff and requires
the user's active approval before being applied - never auto-edited
silently, regardless of how routine the change looks.

**Manual test plan:**
1. `herdr plugin link .`, bind at least two keys with different
   `configuration` overrides (e.g. one plain, one `type="url ipv4"`),
   reload config - confirm each edit to `config.toml` was proposed and
   approved before being applied, not made automatically.
2. From an arbitrary pane with scrollback, press each bound key;
   confirm the popup opens targeting that pane and honors that
   keybind's specific override (filter pre-fill, grab scope).
3. Bind a `tab-scan`-grab key on a tab with 3+ panes; confirm matches
   from every non-floating pane appear, title-prefixed, last-focused
   pane's matches first, and that Insert lands in the launch pane
   regardless of which pane's match was picked.

(`herdr plugin install <owner>/<repo>` against a pushed branch moved to
Phase 10's manual test plan - that's the phase that actually needs a
tagged/pushed release to install against.)

### Phase 8 — 100% config parity (grab profiles, theming, per-type/action overrides)

**Prompt:** Close every remaining gap between this port's `config.toml`
and the original's `zextract.kdl`, verified directly against a fresh
re-read of the original's `doc/config-reference.md` and
`doc/patterns.md` (pulled from `github.com/codingfragments/
zellij-zextract` during Phase 7's follow-up on 2026-08-17, not from
memory - the original has evolved since the Phase 6 gap analysis):

- **`log_level`** top-level scalar (`off`/`error`/`warn`/`info`/`debug`),
  governing stderr diagnostics - currently config parse errors always
  print via a bare `eprintln!` regardless of any setting.
- **`[grab_profiles.<name>]`** - user-definable/overridable named
  scrollback-depth profiles (`source` = `scrollback`/`viewport`/`tab`,
  `lines`, `disable` merged into the disable list *only when this
  profile is active*), replacing `grab.rs`'s hardcoded `PROFILES` const
  as the sole source of profile definitions. Deliberate divergence from
  the original: the original replaces *all four* built-in defaults the
  instant `profiles{}` is present at all ("users who define even one
  profile must list all the profiles they want"); this port instead
  overrides/adds by name only, consistent with how `[profiles.<name>]`
  (Phase 7) already behaves - built-ins for names the user doesn't
  touch survive. `grab::resolve` becomes a `Config` method
  (`resolve_grab_profile`) instead of a pure static lookup.
- **`patterns.command.flag_anchored`** - the original's third
  command-detection strategy (walk back from a `-x`/`--long-flag`
  token to the nearest boundary character), off by default. Not ported
  at all yet - port into `matcher/command.rs`, gated by this key.
- **`[profiles.<name>]` gains a `preview` field**
  (`"on"`/`"off"`/`"always"`/`"never"`) - the original's per-keybind
  `configuration.preview` override, layered on top of this phase's
  `[ui].preview` default. Explicit requirement: preview's default
  state must be configurable through the profile config, not
  hardcoded into the binary or the manifest.
- **`[ui]` block**: `preview` (`"off"`/`"auto"`/`"always"` default
  state - parsed here, consumed by Phase 9's rendering),
  `preview_open_width`/`preview_closed_width`. **`mask_secrets` stays
  excluded** - already decided against on the backburner list above;
  do not re-add it just for parity's sake.
- **`[colors]` block**: full theme override matching every slot in the
  original's table (`muted`/`accent`/`cursor_bg`/`cursor_fg`/
  `highlight`/`error`/`fallback_type` + one `type_*` slot per built-in
  type tag), accepting ANSI name / `#hex` / `rgb(r,g,b)` - wired into
  the picker's actual rendering, not just parsed-and-ignored.
- **`[types.<tag>]` block**: per-type `actions` (verb allow-list,
  replacing Phase 4's static tables) and `default` (Enter verb), with
  the original's hardcoded exceptions preserved (`copy-raw`/`json`
  always allowed for every type; `open`/`edit`/`reveal` always denied
  for `secret` regardless of config).
- **`[actions.<tag>]` block**: command templates for `open`/`edit`/
  `reveal` with the original's full template variable set (`{editor}`
  `{file}` `{line}` `{url}` `{match}` `{raw}` `{display}` `{type}`
  `{context}` `{0..N}`), including the `{line}`-empty separator-
  stripping behavior (`:`/`+`/` ` stripped when `{line}` resolves
  empty, so `"hx {file}:{line}"` degrades to `"hx src/main.rs"` rather
  than `"hx src/main.rs:"`).
- Upgrade Phase 6's hardcoded per-verb dispatch caps to real
  `[limits]` config (`copy`/`insert`/`open`/`edit`/`reveal`/`json`, `0`
  disables a verb entirely).

**Scope:** `config.rs` schema growth (the largest single-phase growth
in the port), `grab.rs` reworked to consult `Config` instead of a
static table, `matcher/command.rs`'s flag-anchored strategy,
`actions.rs`'s template substitution + config-driven allow-lists/caps,
`picker/mod.rs`'s color application, `config.example.toml` +
`doc/config-reference.md` updated in the same commit (per the standing
rule - see memory).

**Out of scope:** preview pane *rendering* itself (Phase 9 - this
phase only adds the config surface Phase 9 reads from). The original's
per-keybind `popupTitle` override has no discovered Herdr equivalent -
Herdr's popup title comes from the static `[[panes]].title` in
`herdr-plugin.toml`, not anything a launch-time env var can override;
document this as a platform gap rather than chasing it as a
deliverable.

**Manual test plan:**
1. Define `[grab_profiles.deep]` with a different `lines` value than
   the built-in; confirm a keybind using it captures that many lines
   instead of the hardcoded 1500. Define a wholly new profile name and
   confirm a `[profiles.<name>]` referencing it by `grab` works without
   touching `grab.rs`.
2. Enable `patterns.command.flag_anchored`; confirm a flag-anchored
   command line (no prompt marker, no known trigger word) now matches.
3. Set `[profiles.url].preview = "always"`; confirm that keybind opens
   with the preview pane already open regardless of `[ui].preview`.
4. Set a custom `[colors]` palette; confirm the picker's tag colors
   and highlight color actually change.
5. Override `[types.url].actions` to drop `open`; confirm `o`/`Ctrl-O`
   on a URL match is now refused.
6. Set `[actions.file].edit` to a custom template; confirm `edit`
   invokes that exact command instead of the hardcoded default.
7. Set `[limits].insert = 1`; confirm a 2-row multi-select insert is
   refused instead of firing.

### Phase 9 — Preview pane

**Prompt:** Port the preview pane: toggled via `p` (List mode) or
`Ctrl-P` (either mode), shows ±3 lines of context around the current
match's location in the captured scrollback text, sized per Phase 8's
`ui.preview_open_width`/`preview_closed_width`.

**Scope:** `picker/mod.rs` split-layout rendering, launch-state
handling for `[ui].preview` (`off`/`auto`/`always`) overridden per
keybind by Phase 8's `[profiles.<name>].preview`
(`on`/`off`/`always`/`never`) when set.

**Out of scope:** multi-pane preview context (which pane's captured
text to show when a tab-scan match is highlighted) beyond whatever
falls out naturally from Phase 7's per-match pane tracking.

**Manual test plan:**
1. `p`/`Ctrl-P` toggles the preview split open/closed.
2. Navigating the list updates the preview to center on the
   highlighted match's line.
3. Confirm the preview pane's width matches
   `ui.preview_open_width`/`preview_closed_width` from config.

### Phase 10 — CI & first release

**Prompt:** Write `.github/workflows/ci.yml` (`cargo fmt --check`,
`cargo clippy -- -D warnings`, `cargo test` on macOS + Linux runners)
and `.github/workflows/release.yml` per §9, including the native
`ubuntu-24.04-arm` runner for `aarch64-unknown-linux-gnu` confirmed in
§12 (no `cross`/QEMU). Cut `v0.1.0` once picker + matcher + actions
parity with the original is manually verified on both a Mac and a Linux
box.

**Scope:** both workflow files, version bump to `0.1.0` +
`CHANGELOG.md` entry via the release-PR flow in `CLAUDE.md`.

**Out of scope:** Option C's prebuilt-binary `install.sh` (stretch,
§8).

**Manual test plan:**
1. Open a PR touching `src/` — confirm `ci.yml` runs and passes on both
   OSes.
2. Merge the release PR, tag `v0.1.0`, push the tag.
3. Confirm `release.yml` runs, produces all four target-triple archives
   + `.sha256` files, and attaches them to the GitHub release.
4. On a clean machine per target triple (or at least one Mac + one
   Linux box), download the release asset and confirm the binary runs.
5. Install via `herdr plugin install <owner>/<repo>` against the
   pushed/tagged branch, confirming the `[[build]]` step actually
   compiles before registration (per §8's "not automatic build
   detection" note) - moved here from Phase 7 since this needs a real
   release to install against.

### Phase 11 — Docs pass

**Prompt:** Update `README.md`'s install section from "planned" to the
real, tested instructions now that `v0.1.0` exists, and port/update
`doc/patterns.md`, `doc/config-reference.md`, `doc/use-cases.md`,
`doc/types.md` from the original repo for Herdr's paths/env vars
(`HERDR_PLUGIN_CONFIG_DIR` etc. instead of the original's Zellij config
location).

**Scope:** README + `doc/` updates only, no code changes.

**Out of scope:** anything not already shipped in v0.1.0 — docs
describe what exists, not aspirational features.

**Manual test plan:**
1. On a machine with no prior state, follow README's install
   instructions verbatim (both Option A and Option B) and confirm each
   works as written.
2. Follow each `doc/use-cases.md` walkthrough manually and confirm the
   described behavior matches what actually happens.

## 12. Open questions (resolved 2026-08-17)

All four verified live against a real Herdr 0.8.0 install (a throwaway
`herdr plugin link`'ed probe plugin, socket API schema dump, and binary
string inspection) — see `git log` on this section for the original
unresolved wording.

- **Focused-pane signal**: `HERDR_PLUGIN_CONTEXT_JSON` reliably includes
  `focused_pane_id` for the pane that was focused *before* the popup
  opened — confirmed live (`nvim` pane `w2:p5` focused → context showed
  `"focused_pane_id":"w2:p5"`). Also carries `focused_pane_cwd`, `tab_id`,
  `workspace_id`, `selected_text`, `clicked_url`, `invocation_source`.
  Correction to §4/§6: the popup process is **not** told its own pane_id
  via env (no `HERDR_PANE_ID`) — only the source pane's id, via context
  JSON.
- **`pane.read` scrollback depth**: confirmed shape — `pane_id`, `source`
  (enum `visible` | `recent` | `recent_unwrapped` | `detection`, wire
  value uses underscores even though the CLI flag uses hyphens), optional
  `lines: u32`, `format` (`text`/`ansi`), `strip_ansi`. `recent_unwrapped`
  is the source to use, matching §4's assumption.
- **Popup singleton behavior**: confirmed, concretely — opening a second
  popup while one is open fails with `"popup already open"`; `pane.list`
  and `api snapshot` never show the popup even while its process is
  alive. Binary strings confirm the rule: "overlay and popup plugin panes
  target the active pane", "popup panes can only open from the normal
  workspace view". Design implication: don't hold a persistent
  pane_id/handle across invocations for "the popup" — each launch is a
  fresh singleton tied to whatever's focused at that moment.
- **`aarch64-unknown-linux-gnu` CI runners**: resolved — no `cross`/QEMU
  needed. GitHub-hosted `ubuntu-24.04-arm`/`ubuntu-22.04-arm` runners are
  GA and, as of 2026-01-29, available in private repos too (previously
  public-only). Use native ARM runners directly in the `release.yml`
  matrix.

## 13. Ideas beyond parity

(Not for v1 — recorded so they aren't lost.)

- Agent-aware actions: e.g. detect if the source pane is an agent pane
  (per Herdr's agent-state tracking) and offer "feed match back into
  agent prompt" as a distinct action from generic pane insert.
- Use `events.subscribe` instead of a one-shot `pane.read` to keep the
  picker's match list live if scrollback changes while the picker is
  open (original plugin snapshots once; could be a nice-to-have).

## License

MIT, matching the original `zextract` project.
