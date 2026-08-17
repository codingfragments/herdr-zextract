# Environment variable reference

`herdr-zextract` reads exactly one environment variable at launch:

| Variable | Purpose | Default |
| --- | --- | --- |
| `ZEXTRACT_PROFILE` | Selects a named profile — grab scope, pattern allowlist, query pre-fill — defined under `[profiles.<name>]` in your own `config.toml` | `open` |

That's the whole surface. Grab scope, pattern filtering, and query
pre-fill used to be set directly via their own env vars
(`ZEXTRACT_GRAB`, `ZEXTRACT_PATTERNS`, `ZEXTRACT_TYPE_FILTER`), which
meant tuning a keybind's behavior meant editing `herdr-plugin.toml` —
this repo's packaging, not something you're meant to change. Those
three are gone; their values now live in `[profiles.<name>]` blocks in
**your own** `config.toml`, keyed by the profile name a keybind's
launcher action selects via `ZEXTRACT_PROFILE`. Full schema and the
four built-in profile names (`open`/`tab`/`url`/`url-tab`) that work
with zero config:
[`doc/config-reference.md`](config-reference.md#profilesname--per-keybind-grab-scope-pattern-allowlist-query-pre-fill).

`ZEXTRACT_PROFILE` itself is set on the launcher action's `command` in
`herdr-plugin.toml`, not on the `[[keys.command]]` binding — confirmed
directly: `[[keys.command]]` (even `type = "plugin_action"`) has no
`env`, `configuration`, or `args` field; `herdr config check` rejects
all three as unknown keys. See
[`doc/keybinding.md`](keybinding.md) for the full binding walkthrough,
including the ten unbound `custom0`..`custom9` slots for combinations
the four shipped profiles don't cover.
