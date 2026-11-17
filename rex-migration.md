# Rex migration plan — one binary, two hosts

> **Status: planned, not started.** This is the active plan for this repo.
> Agents: read this file before starting work, follow the phases in order
> (one `phase/rex-N-…` branch + PR per phase, see CLAUDE.md gitflow), and
> tick the checkboxes / update the "Status" lines as phases land.

## Goal

Make `herdr-zextract` serve **both** [Herdr](https://herdr.dev) and
Rex (superlogical's terminal multiplexer, `com.superlogical.rex`) from the
**same binary, same config, same release**. Herdr keeps working exactly as
today; Rex gets a thin Lua launcher.

```
              herdr-zextract (one binary, one config)
   ┌──────────────────────────────────────────────────┐
   │ matchers · picker · fuzzy · freshness · actions  │
   │                   Host trait                     │
   │        ┌───────────────┴───────────────┐         │
   │   HerdrHost (socket)             RexHost (rex CLI)
   └────────┬────────────────────────────────┬────────┘
   herdr-plugin.toml                plugins/zextract.lua
   (popup pane, actions)            (popup layer, binds — lives in the
                                     user's Rex config, ~/.config/rex)
```

## Background / decisions already made

- **Why not Lua-only?** Rex's config runtime is gopher-lua (Lua 5.1, no
  regex, `io.popen` disabled) and has **no picker/UI primitive** — any
  interactive UI must be a program in a floating layer anyway. Re-writing
  ~5k lines of matchers (block, command, custom regex) in Lua patterns
  would lose features and custom-regex semantics. Rejected.
- **Why not Lua + fzf / headless extractor?** Loses the ratatui picker
  (freshness, type-aware verbs, grab cycle) for no install gain. Rejected.
- **Chosen:** keep the Rust binary, abstract the ~3 herdr socket calls
  behind a `Host` trait, add a Rex backend that shells out to the `rex`
  CLI. Everything else (≈95 % of the code) is host-agnostic already.
- **Distribution:** existing GitHub releases (aarch64-apple-darwin,
  x86_64/aarch64-unknown-linux-gnu) installed via **mise's GitHub
  backend**, pinned in the user's dotfiles:
  ```toml
  [tools]
  "github:codingfragments/herdr-zextract" = "0.7.0"
  ```
  Verified: `mise ls-remote github:codingfragments/herdr-zextract` already
  resolves `0.3.0 … 0.6.0` (plus the rolling `latest` tag — see Phase 5).
  Update = `mise up --bump`. No Rust toolchain on user machines. Fallback
  for hosts without mise: the Lua launcher's popup preflight (see Phase 4).
- **Single installed binary for both hosts:** herdr's manifest stops
  building its own copy and runs `herdr-zextract` from `PATH` (Phase 3).

## Host-coupled surface today (what moves behind the trait)

| Today (herdr) | Where | Rex equivalent |
|---|---|---|
| `pane.read` (`recent_unwrapped` / `visible`, `lines`) | `grab.rs::read_pane` | `rex capture -s S -b B --unwrap` → trim to `lines` in Rust; viewport = last `rows` lines (rows from `rex block call -s S -b B size`, or `REX`-passed rows) |
| `pane.list` + filter by `tab_id` | `grab.rs::capture_tab` | `rex session inspect S --json` → window `W` → its blocks (or `rex block ls -s S -w W --json`); titles from block label / terminal title |
| `pane.send_text` | `actions.rs::insert_text` | `rex send -s S -b B -- TEXT` |
| Launch context (`HERDR_PLUGIN_CONTEXT_JSON` / `HERDR_ACTIVE_PANE_ID`) | `main.rs::launch_context` | `ZEXTRACT_SOURCE_BLOCK`, `ZEXTRACT_SOURCE_WINDOW`, `REX_SESSION` set by the Lua launcher (inside the popup `REX_BLOCK` is the **popup itself**, never the source) |
| `HERDR_SOCKET_PATH` | `main.rs`, `actions.rs` | `REX_CLI` (absolute path of the rex CLI; the Rex server's PATH does not contain it) — default `rex` from PATH |
| Config dir `$HERDR_PLUGIN_CONFIG_DIR/config.toml` | `config.rs` | neutral `~/.config/zextract/config.toml` (Phase 2) |

`socket_client.rs` stays as-is but becomes private to `HerdrHost`.

## Built to move into `muxhost` later

The sister repo `herdr-flash` (see its `rex-migration.md`) will, after
this plan's Phase 3–4 work live, **extract `src/host/` into a shared
crate `codingfragments/muxhost`** that zextract, flash and later nav all
depend on. Nothing here waits for that, but build `src/host/` so the move
is a copy, not a rewrite:

- **Self-contained module:** `src/host/` may use only `std`,
  `serde`/`serde_json` and (for the clipboard helper) `arboard`. Nothing
  from the rest of the crate: no `crate::config`, `crate::matcher`,
  `crate::actions`, no zextract types in signatures. Errors are `String`
  (or one small `host::Error`), and the module doesn't log.
- **Generic env names, tool prefix passed in:**
  `host::detect("ZEXTRACT")` reads `{PREFIX}_HOST`,
  `{PREFIX}_SOURCE_BLOCK` and `{PREFIX}_SOURCE_WINDOW`, so flash can call
  `detect("FLASH")` with the same code. Shared names stay unprefixed:
  `REX_CLI`, `REX_SESSION`, `HERDR_*`.
- **Leave room for flash's needs:** flash also needs colored reads
  (herdr `format=ansi`, Rex `--format=vt`) and optional scroll info.
  - Give `read_pane` a `ReadFormat` parameter (`Plain` now, `Ansi`
    added later), or mark the enums `#[non_exhaustive]`.
  - Any optional capability is a trait method with a default
    implementation (e.g. `scroll_info() -> Ok(None)`), so adding one
    later doesn't break existing implementations.
- **Put the clipboard in the module too:** the arboard + OSC 52 fallback
  from Phase 3 goes in `src/host/clipboard.rs`, because flash copies text
  as well.
- **Tests move with it:** host tests and the fake host live under
  `src/host/` (unit tests) and `tests/fixtures/rex/`, so they can be
  copied over as they are.

## Phases

### Phase 1 — `Host` trait + `HerdrHost` (pure refactor) — `phase/rex-1-host-trait`

Status: ☐ not started

- [ ] New `src/host/mod.rs`:
  ```rust
  pub trait Host {
      fn name(&self) -> &'static str;               // "herdr" | "rex"
      fn source(&self) -> &SourceRef;               // source pane/block + tab/window
      fn read_pane(&self, pane: &str, src: ReadSource, lines: Option<u32>) -> Result<String, String>;
      fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String>; // focused first
      fn insert_text(&self, pane: &str, text: &str) -> Result<(), String>;
  }
  ```
  `ReadSource = Viewport | Scrollback` (maps herdr `visible` / `recent_unwrapped`).
- [ ] `src/host/herdr.rs`: move `socket_client` use, `launch_context`,
  `read_pane`, `pane_title`, `capture_tab` filtering, `insert_text` here.
- [ ] `grab.rs` / `GrabCycler` / `GrabTarget` take `&dyn Host` (or
  `Box<dyn Host>`) instead of `socket_path` + ids. `actions::dispatch`
  gets the host for Insert.
- [ ] `main.rs`: `host::detect("ZEXTRACT")` → `HerdrHost` (only backend so far).
- [ ] No behavior change; `cargo test`, `just check` green; existing grab
  tests ported to a `FakeHost` (makes grab logic unit-testable without a
  socket — a nice side win).

### Phase 2 — neutral config location — `phase/rex-2-config-path`

Status: ☐ not started

- [ ] Config lookup order:
  1. `$ZEXTRACT_CONFIG` (explicit file)
  2. `$XDG_CONFIG_HOME/zextract/config.toml` → `~/.config/zextract/config.toml`
  3. `$HERDR_PLUGIN_CONFIG_DIR/config.toml` (legacy, herdr only)
- [ ] `Ctrl-W` "write starter config" writes to (2) unless only (3) is
  set *and* exists as the user's established location — keep it simple:
  write to (2), mention legacy path in the banner if it's in use.
- [ ] Optional host-scoped overrides: `[host.rex]` / `[host.herdr]`
  tables merged over the root (start with `[host.*.ui]` and
  `[host.*.actions]` only — YAGNI the rest).
- [ ] Docs: `doc/config-reference.md`, `README.md`, `config.example.toml`
  header.

### Phase 3 — `RexHost` + host detection — `phase/rex-3-rex-host`

Status: ☐ not started

- [ ] `src/host/rex.rs`, all calls via `std::process::Command` on
  `$REX_CLI` (default `rex`), always with `-s $REX_SESSION`:
  - `read_pane`: `capture -b B --unwrap --trim`, then keep the last
    `lines` lines (if set). Viewport: last *rows* lines (query `size`
    once via `block call … size`; fall back to the whole capture).
  - `list_tab_panes`: `session inspect $REX_SESSION --json`, pick window
    `ZEXTRACT_SOURCE_WINDOW` (fallback: `active_window_id`), collect its
    terminal blocks (flavor `com.superlogical.terminal*`), **exclude
    floating-layer blocks** (the popup itself), focused/source first.
    Title: block label → terminal title (`block call … title`) → id.
  - `insert_text`: `send -b B -- TEXT`.
- [ ] `host::detect("ZEXTRACT")` (prefix param, see "Built to move into `muxhost`"):
  `ZEXTRACT_HOST=rex|herdr` wins; else `HERDR_SOCKET_PATH` → herdr;
  else `REX_SESSION` → rex; else a clear error listing both.
- [ ] Source must come from `ZEXTRACT_SOURCE_BLOCK` (required under
  Rex — error message points at the Lua launcher).
- [ ] Clipboard: keep `arboard` locally; when the session runs on a
  remote/headless host (no display / arboard error) fall back to
  **OSC 52** written to the popup's tty. Verify Rex passes OSC 52 through
  (Rex exposes `terminal.clipboard_read`, so it intercepts it — spike).
- [ ] `--version` (no clap dependency today — hand-parse, keep it minimal) and
  `--host-api` printing an integer the Lua launcher can check (start at 1).
- [ ] Tests: `RexHost` JSON parsing of `session inspect` fixtures
  (record real output into `tests/fixtures/rex/`), arg construction.
- [ ] **Herdr manifest:** `herdr-plugin.toml` `[[panes]].command =
  ["herdr-zextract"]` (PATH / mise install), drop `[[build]]`. Keep a
  dev note: `just link` still builds locally and puts
  `target/release` first via the justfile, so dev builds keep working.
  *(Check: does herdr's popup inherit the login PATH incl. mise? If not,
  document an absolute path or keep `[[build]]` as an opt-in.)*

### Phase 4 — Rex launcher (`zextract.lua`) — `phase/rex-4-lua-launcher`

Status: ☐ not started

Shipped in this repo as `rex/zextract.lua` (reference implementation);
the user copies/symlinks it into `~/.config/rex/plugins/zextract.lua`
and adds `require("plugins.zextract")` to `init.lua`. The user's Rex
config already has the helpers it builds on: `lib/env.lua` (login PATH,
`rex_cli`) and `lib/popup.lua` (floating layer + `requires` preflight).
The reference file should depend on nothing but `rex.*` and degrade to
inline equivalents if `lib.env` / `lib.popup` are absent.

- [ ] Action `zextract{ profile = "string?" }`:
  1. `rex.session.view{session_id = ctx.session_id}` → active window →
     `focused_block_id` **before** the popup takes focus (same pattern as
     the user's `edit_scrollback`).
  2. Open a floating layer (80 %×80 %, `shell = "none"`) running
     `env PATH=<login PATH> ZEXTRACT_HOST=rex REX_CLI=<abs> \
      REX_SESSION=<sid> ZEXTRACT_SOURCE_BLOCK=<b> ZEXTRACT_SOURCE_WINDOW=<w> \
      ZEXTRACT_PROFILE=<p> herdr-zextract`.
  3. Preflight (`requires`): `command herdr-zextract`, hint
     `mise use -g github:codingfragments/herdr-zextract` ; plus
     `herdr-zextract --host-api` ≥ expected, hint `mise up herdr-zextract`.
- [ ] Bindings (herdr parity, prefix `ctrl+s`):
  `ctrl+s>f` → `open`, `ctrl+s>shift+f` → `tab`,
  `ctrl+s>u` → `url`, `ctrl+s>shift+u` → `url-tab`.
  Custom slots need no packaging under Rex — any `profile` string works
  (`rex.bind("…", "zextract", {profile = "custom0"})`).
- [ ] Verify live (spike list below), especially insert focus-return.
- [ ] Docs: `doc/rex.md` (install via mise, Lua snippet, bindings,
  remote-host notes); README "Hosts" section.

### Phase 5 — distribution polish — `phase/rex-5-distribution`

Status: ☐ not started

- [ ] Rolling `latest` tag/release: mise lists it as a version
  (`mise ls-remote` shows `latest`), which makes `= "latest"` in mise
  ambiguous. Decide: drop the rolling release (it only existed for
  `herdr plugin install --ref latest`, obsolete once herdr runs the PATH
  binary) **or** keep it and document "pin exact versions in mise".
  Recommendation: drop it.
- [ ] Verify `mise use -g github:codingfragments/herdr-zextract@<ver>` on
  macOS arm64 and one Linux host picks the right asset and the binary
  name inside the tarball (`herdr-zextract`).
- [ ] README Install section rewritten: mise first, `cargo install --git`
  as the source fallback, herdr-specific bits under "Herdr".
- [ ] Optional, later: rename binary/repo to `zextract` (GitHub
  redirects; mise tool id changes once). Not part of this migration.

### Phase 6 — release `v0.7.0` — `release/0.7.0`

Status: ☐ not started

- [ ] Version bump + CHANGELOG ("Rex support", "neutral config path",
  "herdr runs PATH binary"), tag per CLAUDE.md release process.
- [ ] Update the user's Rex `migration-plan.md`
  (`~/.config/rex/migration-plan.md`, dotfiles repo — separate PR there):
  move zextract from "impossible" to "done".

## Spike checklist (answer early, record results here)

| # | Question | Result |
|---|---|---|
| S1 | After the floating layer closes, does focus return to the source block (so `rex send` lands where the user expects)? | ☐ |
| S2 | Does Rex pass OSC 52 from a floating-layer terminal to the macOS clipboard (local and remote host)? | ☐ |
| S3 | `rex capture --unwrap` vs herdr `recent_unwrapped`: same line semantics for freshness/offsets? Size/latency on ~100k lines? | ☐ |
| S4 | `session inspect --json` shape: how to tell floating-layer blocks from window blocks; where are block labels/titles? | ☐ |
| S5 | Does herdr's popup inherit a PATH that contains mise installs (`herdr-plugin.toml` with bare `herdr-zextract`)? | ☐ |
| S6 | Remote Rex session: popup runs on the remote host — `open`/`edit`/`reveal` act there. Acceptable, or gate verbs by host? | ☐ |
| S7 | mise github backend picks `herdr-zextract-<triple>.tar.gz` correctly on all three targets | ☐ |

## Rex facts an agent needs (verified 2026-11, rex 0.1.0-preview.19, api rev 14)

- CLI: `rex capture -s S -b B [--unwrap] [--trim] [--format text|html|vt]`,
  `rex send -s S -b B TEXT`, `rex block ls -s S [-w W] --json`,
  `rex session inspect S --json` (`active_window_id`, `windows[].window_id`,
  `windows[].focused_block_id`), `rex block call -s S -b B size|title|pwd`.
- Inside a Rex terminal `REX_SESSION` / `REX_BLOCK` are set; in the popup
  `REX_BLOCK` = the popup's own block.
- The Rex server runs with launchd's minimal PATH; the rex CLI lives at
  `~/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex`
  (the Lua side resolves it; pass it as `REX_CLI`).
- Lua config: `rex.action{name,title,args,run}`, `rex.bind(key, action, args?)`,
  `rex.session.view`, `rex.session.new_layer{bounds={x,y,w,h}, layout=rex.layout.block{flavor="com.superlogical.terminal.shell", options={command=argv, shell="none"}}, focus=true}`,
  `rex.exec{argv}` (sync, `{code, stdout, stderr}`), `rex.json`.
  No UI widgets, no regex, no `io.popen`.
- Reference implementations of the launcher pattern in the user's Rex
  config: `~/.config/rex/plugins/edit_scrollback.lua`, `~/.config/rex/lib/popup.lua`,
  `~/.config/rex/lib/env.lua`.
- Headless test recipe: `rex new selftest && rex do -s selftest selftest.lua && rex session kill selftest`.

## Out of scope

- Windows, Intel-mac release binaries (unchanged policy).
- Porting `herdr-flash` / `herdr-nav`: flash has its own plan
  (`../herdr-flash/rex-migration.md`). That plan creates `muxhost` from
  this repo's `src/host/` and then opens a PR here to switch zextract to
  the crate. Nav follows afterwards with its own plan.
- Creating the `muxhost` crate itself: that belongs to flash's Phase 1,
  not to this plan.
