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
secrets, UUIDs, IPs, **whole code blocks**, and any custom patterns
you configure → you fuzzy-filter, pick, and act:

- **open** a URL in the browser
- **edit** a file at the matched line in your editor
- **copy** the match to the clipboard
- **insert** it back into the source pane's prompt
- **export** a selection as JSON

Code blocks (tag `block`, filter with `#block`) are the snippets
modern codegen CLIs and READMEs show: fenced ` ``` ` blocks, box-
framed sample output (pi-style rounded frames, TUI boxes), and
line-numbered listings (`bat`, `cat -n`, `rg`). A block's value is
its *stripped* content — fence rows, frame rules, side bars, and
gutter prefixes are removed, so copy/insert hands over a clean
snippet. Blocks longer than 200 lines are skipped, and the matches
inside a block (URLs, commands, …) still appear as their own rows.

Every match also carries its *freshness* — how far above the bottom
of its pane buffer it sits (`↑105`, `↑0` = newest line). With equal
fuzzy scores the list orders freshest-first (an empty query means
fully freshness-ordered), and the preview title shows the exact
position (`preview · L1204/1309 · 105↑`) — what tells two
near-identical URLs or commands apart when only a token differs.
Toggle the surfaces via `[ui].freshness_row` /
`[ui].freshness_preview` (each `auto`/`on`/`off`; defaults `auto` —
exactly one surface at a time).

Fills the same gap tmux users cover with `extrakto` / `fingers` /
`fzf-links`.

## Configuration

Optional — the plugin works with zero config. To disable built-in
patterns, tune secret detection, or add your own regex patterns, copy
[`config.example.toml`](config.example.toml) to
`~/.config/zextract/config.toml` (or `$XDG_CONFIG_HOME/zextract/`, or
any path you point `$ZEXTRACT_CONFIG` at). Or launch the plugin with no
config and press `Ctrl-W` in the picker — it writes the same template
for you. The old herdr location (`herdr plugin config-dir
herdr-zextract`) is still read as a fallback. `[host.<name>]` tables
override `[ui]`/`[actions]` for one multiplexer only.

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
`customN` slots and how to add an eleventh action:
[`doc/keybinding.md`](doc/keybinding.md). The one env var involved:
[`doc/env-vars.md`](doc/env-vars.md). The matching layer is documented
in [`doc/types.md`](doc/types.md) (the twelve built-in `#tags`) and
[`doc/patterns.md`](doc/patterns.md) (what each one matches); worked
scenarios in [`doc/use-cases.md`](doc/use-cases.md).

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

### Herdr

```sh
herdr plugin install codingfragments/herdr-zextract            # newest release
herdr plugin install codingfragments/herdr-zextract --ref v0.7.0  # pinned
```

The manifest's `[[build]]` step (`bin/fetch-binary`) downloads the
release binary for that version from GitHub releases (sha256-checked),
so **no Rust toolchain is needed** for tagged versions. Installing from a
branch or untagged commit builds from source with `cargo build
--release` instead (falling back to the release binary, with a warning,
when cargo is missing). `ZEXTRACT_BUILD=source|download` forces one way.

The popup starts `bin/zextract-launch`, which runs the plugin's own
`target/release/herdr-zextract`, or a `herdr-zextract` on `PATH` if the
plugin has none.

For a local checkout: `just link` (builds, then `herdr plugin link .`;
`link` skips `[[build]]`).

### Rex

The same binary, installed with
`mise use -g github:codingfragments/herdr-zextract`, plus the Lua
launcher from [`rex/zextract.lua`](rex/zextract.lua) in your Rex config
(bindings `ctrl+s>f/F/u/U`). Full guide:
[`doc/rex.md`](doc/rex.md).

### From source

```sh
cargo install --git https://github.com/codingfragments/herdr-zextract --tag v0.7.0
```

Requires [Herdr](https://herdr.dev/install.sh) or Rex itself to be
installed first.

## Hosts

One binary, one config (`~/.config/zextract/config.toml`) under both
multiplexers:

- **Herdr:** popup pane via `herdr-plugin.toml`, keybinds through
  plugin actions ([`doc/keybinding.md`](doc/keybinding.md)).
- **Rex:** floating layer via [`rex/zextract.lua`](rex/zextract.lua),
  keybinds `ctrl+s>f/F/u/U` ([`doc/rex.md`](doc/rex.md)).

## Platform support

Built and tested for macOS (Apple Silicon) and Linux (x86_64 +
aarch64). No Windows support planned. No Intel Mac (`x86_64-apple-darwin`)
release binary - build from source with `cargo build --release` if
needed on that architecture.

## License

MIT.
