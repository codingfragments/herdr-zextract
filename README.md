# herdr-zextract

> **Status: planning.** No code yet — see [PLANNING.md](PLANNING.md) for
> the full design and [MIGRATION_FROM_ZELLIJ.md](MIGRATION_FROM_ZELLIJ.md)
> for how this relates to the original Zellij plugin it's based on.

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

Fills the same gap tmux users cover with `extrakto` / `fingers` /
`fzf-links`.

## Build

A native Rust binary — no WASM target involved.

**Requires a working Rust/`cargo` toolchain** (e.g. via
[rustup](https://rustup.rs)) on the machine doing the build.

```sh
git clone https://github.com/codingfragments/herdr-zextract
cd herdr-zextract
cargo build --release
```

Supported targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. Tagged releases
ship prebuilt binaries for all four via GitHub Actions — see
[PLANNING.md §9](PLANNING.md#9-ci--release-plan-github-actions).

## Install

*(Not yet available — both paths below are planned; see
[PLANNING.md §8](PLANNING.md#8-install--distribution-plan) for full
detail.)*

Both options below need a working Rust/`cargo` toolchain on the machine
running the install command — neither is a prebuilt-binary download (that
would be stretch-goal Option C in PLANNING.md, once release automation
exists).

**Option A — `herdr plugin install` (clone + build via the manifest):**
```sh
herdr plugin install codingfragments/herdr-zextract
```
This only builds anything because this repo's `herdr-plugin.toml` will
declare an explicit `[[build]]` step (`cargo build --release`) — Herdr
does not auto-detect Rust projects and build them on its own. No
`[[build]]` declaration means no compilation, regardless of project type.
`herdr plugin link` (for a local checkout) skips `[[build]]` entirely; you
build it yourself first.

**Option B — install a labeled stable release directly onto `PATH`
(recommended once releases exist):**
```sh
cargo install --git https://github.com/codingfragments/herdr-zextract --tag v0.1.0
```
This is `cargo install`, so it also compiles from source (just without a
manifest-driven `[[build]]` step or a repo clone to manage) — a `cargo`
toolchain is required here too. Once installed, point a minimal
`herdr-plugin.toml` at the installed binary and bind a key to it in your
Herdr config.

Requires [Herdr](https://herdr.dev/install.sh) itself to be installed
first.

## Platform support

Built and tested for macOS (Apple Silicon + Intel) and Linux (x86_64 +
aarch64). No Windows support planned.

## License

MIT.
