# herdr-zextract

> **Status: planning.** No code yet — see [PLANNING.md](PLANNING.md) for the
> full design. This repo exists to track the port and collect decisions
> before implementation starts.

A port of [zextract](https://github.com/codingfragments/zellij-zextract)
(a [Zellij](https://zellij.dev) plugin) to run as a plugin for
[Herdr](https://herdr.dev), a persistent terminal runtime built for AI
coding agents.

## What it does (unchanged from the original)

Press a keybind → a picker opens over your terminal → the scrollback of the
previously-focused pane is scanned for URLs, file paths, diagnostics,
commands, secrets, UUIDs, IPs, and any custom patterns you configure → you
fuzzy-filter, pick, and act:

- **open** a URL in the browser
- **edit** a file at the matched line in your editor
- **copy** the match to the clipboard
- **insert** it back into the source pane's prompt
- **export** a selection as JSON

Fills the same gap tmux users cover with `extrakto` / `fingers` / `fzf-links`
— for Herdr instead of Zellij.

## Why a separate repo instead of a fork

Herdr plugins are plain argv binaries talking to a JSON socket API, not WASM
modules on the `zellij-tile` ABI. The host-integration layer is being
rewritten from scratch; the pattern-matching engine, fuzzy filter, and
picker UI are being carried over from the original crate with as few
changes as possible. Once there's meaningful code, it may become a
`herdr` feature/target inside the original repo instead — see
[PLANNING.md](PLANNING.md#relationship-to-the-original-repo) for that
decision point.

## Relationship to the original project

| | |
|---|---|
| Original | [codingfragments/zellij-zextract](https://github.com/codingfragments/zellij-zextract) |
| Original host | Zellij (WASM plugin, `zellij-tile` crate) |
| This repo's host | [Herdr](https://herdr.dev) (native argv plugin, socket API) |
| License | MIT (same as original) |
| Original docs reused | [Built-in patterns](https://github.com/codingfragments/zellij-zextract/blob/main/doc/patterns.md), [Config reference](https://github.com/codingfragments/zellij-zextract/blob/main/doc/config-reference.md), [Types](https://github.com/codingfragments/zellij-zextract/blob/main/doc/types.md) |

## Planned install (not yet available)

Two install paths are planned once a stable release exists — see
[PLANNING.md](PLANNING.md#install--distribution-plan) for full detail:

1. **`herdr plugin install codingfragments/herdr-zextract`** — clones the
   repo, builds from source, registers the plugin. Works on any machine
   with a Rust toolchain.
2. **`cargo install --git https://github.com/codingfragments/herdr-zextract --tag vX.Y.Z`**
   — installs a labeled stable release binary onto `PATH`, then a minimal
   `herdr-plugin.toml` just references the installed binary name. No repo
   clone or build step needed at plugin-registration time.

Prebuilt binaries for macOS (arm64/x86_64) and Linux (x86_64/aarch64) will
be attached to tagged GitHub releases via CI — see PLANNING.md for the
GitHub Actions matrix.

## License

MIT, matching the original `zextract` project.
