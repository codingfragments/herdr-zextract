# Migrating from zellij-zextract

This project is a from-scratch reimplementation of
[zextract](https://github.com/codingfragments/zellij-zextract) targeting
[Herdr](https://herdr.dev) instead of [Zellij](https://zellij.dev). This
doc explains why it's a separate repo, what carries over, and what's being
rewritten.

## Why a separate repo instead of a fork

Herdr plugins are plain argv binaries talking to a JSON socket API, not
WASM modules on the `zellij-tile` ABI. The host-integration layer is being
rewritten from scratch; the pattern-matching engine, fuzzy filter, and
picker UI are being carried over from the original crate with as few
changes as possible. Once there's meaningful code, this may become a
`herdr` feature/target inside the original repo instead of a standalone
project — see [PLANNING.md §12](PLANNING.md#12-open-questions) for that
decision point.

## Relationship to the original project

| | |
|---|---|
| Original | [codingfragments/zellij-zextract](https://github.com/codingfragments/zellij-zextract) |
| Original host | Zellij (WASM plugin, `zellij-tile` crate) |
| This repo's host | [Herdr](https://herdr.dev) (native argv plugin, socket API) |
| License | MIT (same as original) |
| Original docs worth re-reading | [Built-in patterns](https://github.com/codingfragments/zellij-zextract/blob/main/doc/patterns.md), [Config reference](https://github.com/codingfragments/zellij-zextract/blob/main/doc/config-reference.md), [Types](https://github.com/codingfragments/zellij-zextract/blob/main/doc/types.md) |

## What carries over as-is (or close to it)

- The regex-based pattern-matching engine and built-in pattern set (URLs,
  paths, diagnostics, commands, secrets, UUIDs, IPs, custom patterns).
- The `ratatui`-based fuzzy-filter picker UI.
- The type → action mapping (open / edit / copy / insert / export).
- Config schema shape, where it doesn't reference Zellij-specific
  concepts.

## What's being rewritten

Everything that went through `zellij-tile`'s host ABI:

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

See [PLANNING.md](PLANNING.md) for the full architecture, migration
rationale, and open questions being tracked before implementation starts.

## Config/behavior differences to expect

- Zellij's plugin config lived in your `config.kdl`; Herdr plugin config
  lives in a file under the plugin's own config directory
  (`$HERDR_PLUGIN_CONFIG_DIR`) instead — not a single shared multiplexer
  config file.
- The Zellij plugin was pinned to a specific Zellij ABI version (0.44.x).
  Herdr plugins don't have an ABI to pin against — compatibility is
  tracked via `min_herdr_version` in the manifest instead.
- Scrollback depth semantics (`profiles` config: `viewport` vs. N lines)
  need to be re-validated against `pane.read`'s actual parameters — this
  is an open question, not a confirmed 1:1 mapping (see
  [PLANNING.md §12](PLANNING.md#12-open-questions)).
