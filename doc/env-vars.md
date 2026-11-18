# Environment variable reference

`herdr-zextract` reads these environment variables of its own at launch:

| Variable | Purpose | Default |
| --- | --- | --- |
| `ZEXTRACT_PROFILE` | Selects a named profile — grab scope, pattern allowlist, query pre-fill — defined under `[profiles.<name>]` in your own `config.toml` | `open` |
| `ZEXTRACT_CONFIG` | An explicit config file, used instead of the normal lookup (see [`config-reference.md`](config-reference.md#configuration-reference)) | unset |
| `ZEXTRACT_HOST` | Forces the multiplexer host (`herdr` or `rex`) instead of detecting it (`HERDR_SOCKET_PATH` → herdr, else `REX_SESSION` → rex). herdr's `bin/zextract-launch` and the Rex launcher set it explicitly | unset (detected) |
| `ZEXTRACT_SOURCE_BLOCK` | **Rex only, required:** the block (pane) to grab from and insert into. Set by the Rex launcher; inside the popup `REX_BLOCK` is the popup itself | — |
| `ZEXTRACT_SOURCE_WINDOW` | **Rex only:** the source block's window (tab), for tab-wide grabs | found from the session |
| `REX_CLI` | **Rex only:** path of the `rex` CLI (the Rex server's `PATH` doesn't contain it) | `rex` on `PATH` |
| `ZEXTRACT_BUILD` | **Install only** (`bin/fetch-binary`): `source` or `download` forces how the binary is obtained | auto |

Day to day only `ZEXTRACT_PROFILE` matters; the others are set by the
launchers or are for unusual setups and testing.

The binary also answers two flags without starting the picker:
`--version` (`herdr-zextract 0.6.0`) and `--host-api` (an integer the
Rex launcher checks to warn about a binary that is too old).

Grab scope, pattern filtering, and query
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
