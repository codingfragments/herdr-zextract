# herdr-zextract

[Releases](https://github.com/codingfragments/herdr-zextract/releases) ·
[PLANNING.md](PLANNING.md) for the full design and phase history ·
[MIGRATION_FROM_ZELLIJ.md](MIGRATION_FROM_ZELLIJ.md) for how this
relates to the original Zellij plugin it's ported from.

A [Herdr](https://herdr.dev) plugin that extracts typed matches from your
focused pane's scrollback and presents them in a fuzzy-filterable picker
with type-aware actions.

## What it does

Press a keybind → a picker opens → the scrollback of the previously
focused pane is scanned for URLs, file paths, diagnostics, commands,
secrets, UUIDs, IPs, and any custom patterns you configure → you
fuzzy-filter, pick, and act:

- **open** a URL in the browser
- **edit** a file at the matched line in your editor
- **copy** the match to the clipboard
- **insert** it back into the source pane's prompt
- **export** a selection as JSON

`Ctrl-G` cycles the running picker through every configured grab
profile (quick/deep/viewport/full-scrollback/whole-tab, or your own)
without relaunching; `p`/`Ctrl-P` toggles a preview split showing
context around the highlighted match.

Fills the same gap tmux users cover with `extrakto` / `fingers` /
`fzf-links`.

## Docs

| Doc | Covers |
|---|---|
| [`doc/config-reference.md`](doc/config-reference.md) | Full `config.toml` schema |
| [`doc/keybinding.md`](doc/keybinding.md) | Shipped actions, binding a key, adding your own |
| [`doc/patterns.md`](doc/patterns.md) | How each built-in `#type` is detected |
| [`doc/types.md`](doc/types.md) | Verbs/template-fields/color quick reference per type |
| [`doc/use-cases.md`](doc/use-cases.md) | Worked walkthroughs |
| [`doc/env-vars.md`](doc/env-vars.md) | The one env var involved |

## Configuration

Optional — the plugin works with zero config. To disable built-in
patterns, tune secret detection, or add your own regex patterns, copy
[`config.example.toml`](config.example.toml) to:

```sh
herdr plugin config-dir herdr-zextract   # prints the target directory
```

as `config.toml`. Or launch the plugin with no config and press
`Ctrl-W` in the picker — it writes the same template for you.

Full schema: [`doc/config-reference.md`](doc/config-reference.md).

## Keybinding

The plugin ships fourteen ready-to-bind actions (four with built-in
defaults — current-pane vs. whole-tab grab, all-types vs. URL/IP-only —
plus ten free `customN` slots), bound via `[[keys.command]]` entries
with `type = "plugin_action"` in your own
`~/.config/herdr/config.toml` — Herdr owns all keybindings, the plugin
never binds its own keys. Each action just names a *profile*; the
profile's actual grab scope and pattern filter live in your own
`config.toml` under `[profiles.<name>]` (see Configuration above), not
in the plugin's packaging. Full binding reference, including the
`customN` slots and how to add an eleventh action, is in
[`doc/keybinding.md`](doc/keybinding.md) (see the Docs table above).

## Build

A native Rust binary — no WASM target involved.

**Requires a working Rust/`cargo` toolchain** (e.g. via
[rustup](https://rustup.rs)) on the machine doing the build.

```sh
git clone https://github.com/codingfragments/herdr-zextract
cd herdr-zextract
cargo build --release
```

Supported targets: `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`. Tagged releases ship prebuilt binaries for
all three via GitHub Actions — see
[PLANNING.md §9](PLANNING.md#9-ci--release-plan-github-actions).

## Install

Requires [Herdr](https://herdr.dev/install.sh) itself, and a working
Rust/`cargo` toolchain on the machine running the install command —
both options below build from source (there's no prebuilt-binary
`install.sh` yet; GitHub Releases attach prebuilt binaries per target
triple, but nothing consumes them automatically today).

**Option A — `herdr plugin install` (recommended):**
```sh
herdr plugin install codingfragments/herdr-zextract --ref v0.1.1
```
Clones the repo at that ref, runs the `[[build]]` step
(`cargo build --release`) `herdr-plugin.toml` declares, and registers
the plugin — one command, no separate build step. Pin `--ref` to a
tagged version (`v0.1.1`) for a reproducible install, or use
`--ref latest` to track whatever the newest tagged release is (a
rolling tag, force-moved on every release — see
[CHANGELOG.md](CHANGELOG.md) for what's in it). Omit `--ref` entirely
to track `main`. Add `--yes` to skip the confirmation prompt (needed
for non-interactive/scripted installs, e.g. from dotfiles).

To update later, just re-run the same command — it re-resolves the
ref and rebuilds in place; there's no separate `herdr plugin update`.

**Option B — install a labeled release directly onto `PATH`:**
```sh
cargo install --git https://github.com/codingfragments/herdr-zextract --tag v0.1.1
```
This is `cargo install`, so it also compiles from source — just
without a manifest-driven `[[build]]` step or a repo clone to manage.
Point a minimal `herdr-plugin.toml` at the installed binary
(`command = ["herdr-zextract"]`, resolved via `PATH`) and bind a key to
it in your Herdr config.

Once installed, bind a key — see [`doc/keybinding.md`](doc/keybinding.md).

## Platform support

Built and tested for macOS (Apple Silicon) and Linux (x86_64 +
aarch64). No Windows support planned. No Intel Mac (`x86_64-apple-darwin`)
release binary - build from source with `cargo build --release` if
needed on that architecture.

## License

MIT.
