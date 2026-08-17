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

- Target triples: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`.
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

Planned workflow, `.github/workflows/release.yml` (not yet written):

- Trigger: tag push matching `v*.*.*`.
- Matrix:
  - `macos-14` (arm64, native) → `aarch64-apple-darwin`
  - `macos-13` (x86_64, native) → `x86_64-apple-darwin`
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
phase — real keybinding wiring is Phase 6).

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

### Phase 6 — Manifest polish & keybinding

**Prompt:** Finalize `herdr-plugin.toml` for both install paths in §8
(Option A's `[[build]]`-driven install, Option B's `cargo install`
minimal manifest), and wire a real Herdr keybind (`[[actions]]` in the
manifest, per the schema found in Phase 1's investigation) so the
plugin launches via keypress against whatever pane is focused, not just
via `herdr plugin pane open` from the CLI.

**Scope:** final manifest shape, README's "Option A/B" install
instructions validated against both paths.

**Out of scope:** CI/release automation (Phase 7).

**Manual test plan:**
1. `herdr plugin link .`, bind a key to the plugin action in Herdr
   config, reload config.
2. From an arbitrary pane with scrollback, press the bound key.
3. Confirm the popup opens targeting that pane (not whatever was
   focused when the plugin was linked).
4. Repeat via `herdr plugin install <owner>/<repo>` against a pushed
   branch, confirming the `[[build]]` step actually compiles before
   registration (per §8's "not automatic build detection" note).

### Phase 7 — CI & first release

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

### Phase 8 — Docs pass

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
