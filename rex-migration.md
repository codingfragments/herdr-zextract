# Rex migration plan — one binary, two hosts

> **Status: Phases 1–4 done (Phase 4 PR pending); Phase 5 next.** This is the active plan for this repo.
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
- **Two install paths, one release (decided 2026-11, "option B"):**
  `herdr plugin install` must keep working on its own, without mise.
  - **herdr:** `herdr plugin install codingfragments/herdr-zextract`
    stays a single command. Its `[[build]]` step runs
    `bin/fetch-binary`, which downloads the release binary matching the
    checked-out version into `target/release/` (sha256-checked) and
    falls back to `cargo build --release` when no release asset matches.
    Tagged versions therefore need no Rust.
  - **Rex:** `mise use -g github:codingfragments/herdr-zextract`.
  - The herdr manifest starts `bin/zextract-launch`, which prefers the
    plugin's own `target/release/herdr-zextract`, then falls back to
    `herdr-zextract` on `PATH`. `just link` (dev) keeps working because
    it builds into `target/release/` first.
  - Trade-off: herdr and Rex each have their own copy and can drift in
    version; update both (`herdr plugin install --ref …` and `mise up`).
  - The earlier idea, herdr running only the `PATH` binary with no
    `[[build]]`, was dropped: it would have made `herdr plugin install`
    depend on mise.

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

Status: ☑ done (PR #38)

- [x] New `src/host/mod.rs`: `Host` trait (`name`, `source`, `read_pane`,
  `list_tab_panes`, `insert_text`), `SourceRef { pane_id, tab_id }`,
  `PaneInfo { pane_id, title }`, `ReadSource = Viewport | Scrollback`
  (`#[non_exhaustive]`, maps herdr `visible` / `recent_unwrapped`), and
  `detect(prefix)`. No `ReadFormat` parameter yet: flash adds colored
  reads when it creates `muxhost`; the non-exhaustive enum keeps that
  addition cheap.
- [x] `src/host/herdr.rs`: `launch_context` (now pure, env values passed
  in), `read_pane`, `pane_title`, the tab filter + source-first ordering
  (`panes_on_tab`) and `insert_text`. `socket_client.rs` moved to
  `src/host/` and is private to the module; its request id now uses
  `CARGO_PKG_NAME` instead of a hard-coded `herdr-zextract`.
- [x] `grab.rs`: `GrabTarget` is gone; `GrabCycler::new` takes
  `Rc<dyn Host>` (shared with `main.rs`, which still needs the host for
  Insert after the picker, which owns the cycler, returns).
  `capture(profile, &dyn Host)`. `actions::dispatch` / `execute_batch`
  take `&dyn Host`; Insert targets `host.source().pane_id`.
- [x] `main.rs`: `host::detect("ZEXTRACT")` → `HerdrHost` (`ZEXTRACT_HOST=herdr`
  explicit, else `HERDR_SOCKET_PATH`); debug log line names the host.
- [x] No behavior change: error messages unchanged (`grab failed: pane.read
  failed: …`, `insert failed: …`). `just check` green (297 tests; new
  `FakeHost` in `src/host/fake.rs` with tests for capture, tab capture,
  pane tagging, insert, launch context, tab filtering). Checked live
  against a running herdr: `open` and `tab` profiles capture and reach
  the picker; a bad socket still fails at the grab.
- [x] `src/host/` is self-contained (no `crate::` imports), ready to move
  into `muxhost`.

### Phase 2 — neutral config location — `phase/rex-2-config-path`

Status: ☑ done (PR #39)

- [x] Config lookup order (`config::locate`, pure and unit-tested; first
  existing file wins):
  1. `$ZEXTRACT_CONFIG` (explicit file; when set, no fallback)
  2. `$XDG_CONFIG_HOME/zextract/config.toml` (absolute only, per XDG) →
     `~/.config/zextract/config.toml`
  3. `$HERDR_PLUGIN_CONFIG_DIR/config.toml` (legacy, herdr only)
- [x] `Ctrl-W` writes to (1) if set, else (2). The config-missing banner
  now shows whenever no file exists anywhere (previously only when
  `HERDR_PLUGIN_CONFIG_DIR` was set). No banner note for the legacy
  path: when the legacy file is in use the config isn't missing, so the
  banner never shows; the docs cover moving it instead.
- [x] Host-scoped overrides: `[host.<name>]` with `ui` and `actions`
  only, deep-merged over the root at the TOML level before
  deserializing (`apply_host_overrides`); other sections under
  `[host.<name>]` are ignored with a `log_level = warn` message.
  `Config::load(host.name())`; parsing split into
  `Config::from_toml_str(text, host)` for tests.
- [x] Docs: `doc/config-reference.md` (lookup order + new
  `[host.<name>]` section), `doc/env-vars.md` (`ZEXTRACT_CONFIG`,
  `ZEXTRACT_HOST`), `README.md`, `config.example.toml` header.
- [x] `just check` green (307 tests).
- **Note for Rex (Phase 4 docs):** `HERDR_PLUGIN_CONFIG_DIR` is not set
  under Rex, so a config that only exists at the legacy herdr location
  is invisible there. Move it to `~/.config/zextract/config.toml` (herdr
  keeps finding it there too).

### Phase 3 — `RexHost` + host detection — `phase/rex-3-rex-host`

Status: ☑ implemented on `phase/rex-3-rex-host` (PR pending; two live
checks left for the user, see the end of this section)

- [x] `src/host/rex.rs`, all calls through the `rex` CLI (`$REX_CLI`,
  default `rex`) behind a small `RexRunner` trait (tests use a fake that
  records the exact arguments):
  - Scrollback: `capture -s S -b B --unwrap --trim`, last `lines` lines.
  - Viewport: `capture -s S -b B` (no unwrap: screen rows are wrapped
    rows), last *rows* lines from `block call … size`; whole capture if
    `size` fails.
  - `list_tab_panes`: `session inspect S --json`; window =
    `ZEXTRACT_SOURCE_WINDOW`, else the window holding the source block,
    else `active_window_id`; blocks of `kind = "tiled"` layers whose
    `creator_name` starts with `com.superlogical.terminal`; floating
    layers (the popup) excluded; source first.
  - Titles: a user-set label; Rex labels every new shell block `shell`,
    so for that label the terminal title (`block call … title`) is used,
    then the label, then `pane <id>`.
  - `insert_text`: `send -s S -b B` with the text on **stdin**, so text
    starting with `-` or containing newlines arrives exactly.
- [x] `host::detect("ZEXTRACT")`: `ZEXTRACT_HOST=herdr|rex` wins; else
  `HERDR_SOCKET_PATH` → herdr; else `REX_SESSION` → rex; else an error
  naming both.
- [x] `ZEXTRACT_SOURCE_BLOCK` is required under Rex; the error points at
  the Lua launcher. (Verified live: once the popup layer opens, the
  window's `focused_block_id` *is* the popup, so there is no safe
  fallback.)
- [x] Clipboard: `src/host/clipboard.rs` - `arboard`, falling back to
  OSC 52 on `/dev/tty` when there is no system clipboard (headless /
  remote). Hand-rolled base64 (RFC 4648 vectors tested), no new
  dependency. Whether Rex forwards OSC 52 from a floating layer to the
  Mac clipboard stays spike S2 (Phase 4).
- [x] `--version` / `-V` and `--host-api` (`host::HOST_API = 1`),
  answered before host detection.
- [x] Tests: `tests/fixtures/rex/session-inspect.json` recorded from a
  live Rex session (two windows, two tiled shells, one floating popup);
  13 Rex tests (window choice, popup exclusion, titles, line caps,
  exact CLI arguments, stdin insert, error pass-through). `just check`
  green, 320 tests.
- [x] **Live test in Rex** (throwaway session, binary run in a floating
  layer like the launcher will): the `tab` profile listed both tiled
  panes and not the popup, the terminal title showed for the generic
  label, and `Tab` + `i` inserted the URL into the source block.
- [x] **Herdr install, option B:**
  - `bin/fetch-binary`: tagged commit (or no git info) → download the
    `v<version>` release asset for the platform, sha256-check, unpack
    into `target/release/`; on failure → `cargo build --release`.
    Untagged commit → `cargo build --release` (the release binary would
    be older than the code); without cargo → the release binary with a
    warning. `ZEXTRACT_BUILD=source|download` forces one way. Tested:
    a `v0.6.0` clone downloads and installs the arm64 binary; an
    untagged commit without cargo falls back to the download with the
    warning; a missing release fails cleanly with the URL.
  - `bin/zextract-launch`: `target/release/herdr-zextract`, else
    `herdr-zextract` on `PATH`, else an install hint that waits for
    Enter. It also sets `ZEXTRACT_HOST=herdr` itself, so the launcher
    actions stay unchanged (herdr's `--help` doesn't say whether
    `--env` may be repeated).
  - `herdr-plugin.toml`: `[[build]] command = ["./bin/fetch-binary"]`,
    `[[panes]].command = ["./bin/zextract-launch"]` (relative like the
    old `./target/release/…` path, scripts are committed executable).
- [x] Docs: `README.md` Install section rewritten (herdr, Rex, source),
  `doc/env-vars.md` (Rex env vars, `ZEXTRACT_BUILD`, the two flags).
- [ ] **Left for the user to verify in herdr** (both change the user's
  installed plugin, so not done automatically):
  1. With the linked checkout (`just link`), the popup still opens via
     `bin/zextract-launch`.
  2. `herdr plugin install codingfragments/herdr-zextract --ref <tag>`
     runs `bin/fetch-binary` as its `[[build]]` step. Only testable
     once a release contains the scripts (Phase 6); until then
     `--ref phase/rex-3-rex-host` exercises the source-build path.

### Phase 4 — Rex launcher (`zextract.lua`) — `phase/rex-4-lua-launcher`

Status: ☑ implemented and live-tested on `phase/rex-4-lua-launcher`
(PR pending)

- [x] `rex/zextract.lua`: self-contained launcher; uses `lib.env`
  (login PATH, `rex_cli`) when present, else inline fallbacks (default
  PATH, the app-bundle rex CLI). Action `zextract{profile = "string?"}`:
  `rex.session.view` → active window → `focused_block_id` before the
  layer takes focus; floating layer 80 %×80 %, `shell = "none"`; env
  `ZEXTRACT_HOST=rex REX_CLI REX_SESSION ZEXTRACT_SOURCE_BLOCK
  ZEXTRACT_SOURCE_WINDOW ZEXTRACT_PROFILE`. The preflight (POSIX sh,
  inside the layer, so it also names the remote host) checks
  `command -v herdr-zextract` (hint `mise use -g …`) and `--host-api`
  (hint `mise up herdr-zextract`), then `exec`s the binary.
- [x] Bindings: `ctrl+s>f` open, `ctrl+s>shift+f` tab, `ctrl+s>u` url,
  `ctrl+s>shift+u` url-tab; verified with `rex keymap` (81 bindings
  after reload). Custom profiles need no packaging.
- [x] **Live-tested** in a throwaway session with the launcher installed
  in the user's Rex config (dotfiles branch `feat/rex-zextract-plugin`):
  - `open` profile: popup over the source pane, picker populated from
    its scrollback.
  - insert: typed the picked URL into the source pane's prompt and
    closed the popup.
  - **Spike S1 answered — yes:** after the floating layer closes, the
    window's `focused_block_id` is back on the source block, so typing
    after the popup lands in the right pane.
  - `tab` profile: matches from both panes of a split, popup excluded.
- [x] Docs: `doc/rex.md` (install, bindings, remote-host notes, limits),
  README "Hosts" + Rex install sections.
- Spike S2 (OSC 52 through a Rex layer) still open: on the dev Mac the
  system clipboard answers first, so the fallback path isn't exercised;
  matters only for remote/headless hosts.
- The binary is not yet mise-installed; for testing the dev build is
  symlinked to `~/.local/bin/herdr-zextract`. Phase 5 replaces that
  with the real mise install.

### Phase 5 — distribution polish — `phase/rex-5-distribution`

Status: ☐ not started

- [ ] Rolling `latest` tag/release: **keep it** (option B). `herdr plugin
  install --ref latest` (`just install`) still uses it, and
  `bin/fetch-binary` resolves the binary from the version in that
  ref's `Cargo.toml`, so it gets the matching release. mise lists
  `latest` as a version, so document "pin exact versions in mise"
  instead of using `= "latest"`.
- [ ] Verify `mise use -g github:codingfragments/herdr-zextract@<ver>` on
  macOS arm64 and one Linux host picks the right asset and the binary
  name inside the tarball (`herdr-zextract`).
- [ ] README Install section rewritten: "Herdr" (`herdr plugin install`,
  no Rust needed for tagged versions) and "Rex" (mise), with
  `cargo install --git` as the source fallback.
- [ ] Optional, later: rename binary/repo to `zextract` (GitHub
  redirects; mise tool id changes once). Not part of this migration.

### Phase 6 — release `v0.7.0` — `release/0.7.0`

Status: ☐ not started

- [ ] Version bump + CHANGELOG ("Rex support", "neutral config path",
  "herdr plugin install downloads the release binary"), tag per CLAUDE.md
  release process.
- [ ] Update the user's Rex `migration-plan.md`
  (`~/.config/rex/migration-plan.md`, dotfiles repo — separate PR there):
  move zextract from "impossible" to "done".

## Spike checklist (answer early, record results here)

| # | Question | Result |
|---|---|---|
| S1 | After the floating layer closes, does focus return to the source block (so `rex send` lands where the user expects)? | ☑ 2026-11, Phase 4 live test: yes |
| S2 | Does Rex pass OSC 52 from a floating-layer terminal to the macOS clipboard (local and remote host)? | ☐ |
| S3 | `rex capture --unwrap` vs herdr `recent_unwrapped`: same line semantics for freshness/offsets? Size/latency on ~100k lines? | ☐ |
| S4 | `session inspect --json` shape: how to tell floating-layer blocks from window blocks; where are block labels/titles? | ☐ |
| S5 | Does herdr's popup inherit a PATH that contains mise installs (`herdr-plugin.toml` with bare `herdr-zextract`)? | ☑ 2026-11: yes on the user's Mac — the herdr server processes carry the login PATH incl. `~/.local/share/mise/shims`, so later `mise use -g` installs are found without restarting herdr. Now only relevant for `zextract-launch`'s PATH fallback (option B). |
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
