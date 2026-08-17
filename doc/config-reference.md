# Configuration reference

`herdr-zextract` reads `$HERDR_PLUGIN_CONFIG_DIR/config.toml` once at
startup (config is a per-invocation snapshot — no live reload while the
popup is open). Find your config dir with:

```sh
herdr plugin config-dir herdr-zextract
```

If the file is missing, or fails to parse, built-in defaults are used —
a broken config never crashes the plugin (a parse error is reported on
stderr, visible in `herdr plugin log`).

**Template:** [`config.example.toml`](../config.example.toml) at the
repo root is a ready-to-copy starter with every key from this doc,
commented out at its default value. It's also what `main.rs` embeds
verbatim (`include_str!`) as the file the picker's `Ctrl-W` action
writes when no config exists yet — one source of truth, so the
template can't drift from what the plugin actually ships.

**Format note:** the original `zellij-zextract` plugin this is ported
from uses KDL, matching Zellij's own config format. This port uses TOML
instead, matching Herdr's own config conventions (`config.toml`,
`herdr-plugin.toml`).

**Scope note:** the `patterns { }` block and `[profiles.<name>]` below
are ported so far (PLANNING.md §11 Phases 5 and 7). The original's
`ui`, `colors`, `limits`, `types`, and `actions` blocks — theming,
per-verb caps, and action-template overrides — aren't built yet; this
doc will grow as later phases add them.

---

## `[patterns]` — global disable list

```toml
[patterns]
disable = ["secret", "ipv6"]
```

`disable` accepts one or more built-in type tags or custom pattern
names. Listed patterns are skipped entirely, for every invocation.

**Built-in type tags:** `url`, `file`, `diag`, `git`, `sha`, `ipv4`,
`ipv6`, `uuid`, `quote`, `cmd`, `secret`

---

## `[patterns.secret]` — entropy-fallback toggle

```toml
[patterns.secret]
entropy_filter = true   # default
```

`secret` detection runs two passes: curated regexes for known formats
(JWT, AWS, GitHub, GitLab, Stripe, OpenAI, Anthropic, Slack, Bearer —
these always run regardless of this setting) plus an entropy-based
fallback that catches unknown high-entropy tokens. Set `entropy_filter
= false` to disable the fallback pass and rely only on curated formats
(fewer false positives, misses unknown secret formats).

---

## `[[patterns.custom]]` — user-defined regex patterns

Each custom pattern gets its own display label in the picker and is
filterable with `#name`, just like a built-in type tag.

| Key | Required | Description |
|---|---|---|
| `name` | yes | Display label and `#name` filter tag. |
| `regex` | yes | Regular expression (`regex-lite` syntax — no lookaround). Invalid patterns are silently skipped, so a typo in one pattern can't break the others. |
| `type` | no | Which built-in `MatchType` governs this pattern's verb allow-list/color/default-verb (see `doc/types.md`). Default: `"url"`. Unknown type strings fall back to `"url"`. |
| `template` | no | Template string applied to the match, building `raw`/`display`. Omit to use the matched text as-is. |

### Capture group semantics

- **No groups** — `{match}` = full regex match.
- **One or more groups** — `{match}` = group 1; `{1}` = group 1, `{2}`
  = group 2, etc.; `{0}` = full match.

When `template` is present, `raw` (the value copy/insert/export act
on) is the expanded template result, not the raw regex match text.

**Example — matches the manual test plan in PLANNING.md §11 Phase 5:**

```toml
[patterns]
disable = ["ipv6"]

[[patterns.custom]]
name = "port"
regex = ":[0-9]{4,5}\\b"
type = "url"

[[patterns.custom]]
name = "jira"
regex = "([A-Z]+)-([0-9]+)"
type = "url"
template = "https://jira.example.com/browse/{1}-{2}"

[[patterns.custom]]
name = "github-pr"
regex = "github\\.com/([^/\\s]+)/([^/\\s]+)/pull/([0-9]+)"
type = "url"
template = "https://github.com/{1}/{2}/pull/{3}"
```

With this config, scrollback containing `PROJ-123` shows a `[jira]`
match whose `raw`/`display` is
`https://jira.example.com/browse/PROJ-123` — type-aware actions (open,
copy, insert) all operate on that expanded URL, not the literal
`PROJ-123` text.

---

## `[profiles.<name>]` — per-keybind grab scope, pattern allowlist, query pre-fill

```toml
[profiles.tab]
grab = "tab-scan"

[profiles.url]
patterns = ["url", "ipv4", "ipv6"]
```

Every keybind's launcher action (in `herdr-plugin.toml`) sets
`ZEXTRACT_PROFILE=<name>` — that name is looked up here at launch. This
is the layer meant for you to tune per keybind; `herdr-plugin.toml`
itself only ever names a profile, never carries the grab/pattern
values directly. See [`doc/keybinding.md`](keybinding.md) for how a
profile name gets selected by a keybind.

| Key | Description |
|---|---|
| `grab` | Scrollback-depth / scan-scope profile: `quick` (150 lines, current pane — default), `deep` (1500 lines), `viewport` (visible screen only), `full` (entire scrollback), `tab-scan` (every pane on the current tab, 150 lines each, last-focused pane first). Unrecognized values fall back to `quick`. |
| `patterns` | Allowlist of type tags (built-in or custom) to extract at all for this profile, overriding `[patterns].disable` entirely. Omit for no restriction (the config's own `disable` list still applies). |
| `type_filter` | Type tags to pre-fill the picker's query with as `#tag` filters — narrows what's shown, doesn't restrict what's extracted (unlike `patterns`). |

**Built-in profiles:** `open`, `tab`, `url`, and `url-tab` have
built-in Rust-side defaults matching the four actions
`herdr-plugin.toml` ships (see the table in
[`doc/keybinding.md`](keybinding.md#shipped-actions)), so those four
keybinds work with no config file at all. Defining `[profiles.open]`
(etc.) here replaces that built-in entirely — it doesn't merge with
it, so restate every key you want, not just the one you're changing.

**Custom profiles:** `herdr-plugin.toml` also ships ten unbound slots,
`custom0` through `custom9`, with no built-in default — undefined ones
degrade to plain defaults (`quick` grab, no restriction) rather than
failing, so binding a `zextract-customN` action before configuring its
profile is safe. Define `[profiles.customN]` here to give one an actual
grab scope and/or pattern allowlist, then bind a key to the matching
`zextract-customN` action.
