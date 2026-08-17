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

**Scope note:** every block below is ported as of PLANNING.md §11 Phase
8 — `log_level`, `[grab_profiles.<name>]`, `patterns.command.flag_anchored`,
`[ui]`/`[profiles.<name>].preview`, `[colors]`, and `[types]`/
`[actions]`/`[limits]` all reached 100% parity with the original's
`zextract.kdl` surface in that phase. Preview pane *rendering* itself
is still Phase 9 — `[ui].preview` is parsed and resolved but has
nothing to render against yet.

---

## `log_level` — stderr diagnostic verbosity

```toml
log_level = "info"   # default
```

One of `off`, `error`, `warn`, `info`, `debug`. Governs
`herdr-zextract`-prefixed stderr diagnostics (visible via `herdr plugin
log`) - not the picker's own user-facing result/error line, which
always shows regardless (it's the only feedback a failed launch gets).
`off` shows nothing; `debug` shows everything, including which grab
profile and pattern restriction a launch resolved to.

One inherent limitation: a config *parse error* is reported at the
default level regardless of this setting, since the level itself lives
in the same file that failed to parse.

---

## `[ui]` — preview pane sizing and default state

```toml
[ui]
preview = "off"                # default
preview_open_width = "40%"     # default
preview_closed_width = "70%"   # default
```

| Key | Description |
|---|---|
| `preview` | Preview pane state at launch. `"off"` = closed (default), `"auto"` = closed (see note below), `"always"` = open. Overridden per keybind by `[profiles.<name>].preview` when set. |
| `preview_open_width` | The list column's width while the preview is open - a percent string (`"40%"`) or a bare cell count (`"120"`). The preview column takes whatever's left (60% by default). |
| `preview_closed_width` | Has no effect (see note below). |

**Note on `"auto"`:** the original remembers the previous session's
open/closed state across launches. This port has no such persistence —
each invocation is a fresh process with nothing to remember — so
`"auto"` behaves identically to `"off"` here.

**Note on width and `preview_closed_width`:** the original resizes the
*whole popup* wider when the preview opens (a real floating-pane
resize). Herdr's popup pane is a fixed-size PTY set once at launch by
`herdr-plugin.toml`, with no live-resize equivalent wired up here, so
this port instead splits its own fixed render area into list+preview
columns - `preview_open_width` sizes the list column of that split.
`preview_closed_width` was the original's *closed* popup width; with
no split at all when preview is closed, there's nothing for it to
size, so it's accepted (parses without error) but otherwise ignored.

Toggle the split with `p` (List mode) or `Ctrl-P` (either mode). The
preview shows up to 3 lines before and after the highlighted match's
line in the pane it came from, with the match's own line highlighted -
and within that line, the exact extracted text picked out with an
inverted highlight, since a match is rarely the whole line.

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

## `[patterns.command]` — flag-anchored detection toggle

```toml
[patterns.command]
flag_anchored = false   # default
```

`cmd` detection runs two strategies always (prompt-anchored, then
exec-anchored against a known trigger word) plus a third, opt-in
strategy tried only when neither of the first two matches a line: find
the leftmost standalone `-x`/`-xyz`/`--long-flag` token, walk backward
to the nearest boundary character (`][}{><:;|&(,'"`), then forward past
whitespace to the command word. Catches commands with no recognized
prompt and no trigger word (e.g. `[dry-run] jq -r '.foo' file.json`).

Guards: the command word must start with a lowercase ASCII letter and
be at least 2 characters — rejects `The --verbose flag` (uppercase) and
single-letter noise. Off by default because it can still false-positive
on ordinary prose containing flag-looking tokens (e.g. `"missing
argument -v"`); use `#cmd`/`#!cmd` query filters to show or hide command
matches in a session where the noise is too high.

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
| `preview` | Force the preview pane open/closed for this keybind specifically, overriding `[ui].preview`'s default. One of `on`, `off`, `always`, `never`. Omit to use the `[ui]` default. |

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

---

## `[grab_profiles.<name>]` — grab profile definitions

```toml
[grab_profiles.deep]
lines = 3000

[grab_profiles.jira-deep]
source = "scrollback"
lines = 500
disable = ["secret"]
```

`jira-deep` above is a wholly custom name — none of the built-in five
(`quick`/`deep`/`viewport`/`full`/`tab-scan`) is named that. Defining a
block under any new name is enough to make it a selectable grab
profile; wire it up by pointing a `[profiles.<name>].grab` at it, e.g.

```toml
[profiles.custom0]
grab = "jira-deep"
```

Commented out (deactivated) in `config.example.toml` by default —
uncomment both blocks to use it.

Don't confuse this with `[profiles.<name>]` above: that block only ever
*selects* a grab profile by name (`grab = "deep"`); this block defines
what a grab profile name actually *means*. `quick`/`deep`/`viewport`/
`full`/`tab-scan` all have built-in Rust-side definitions, so grab
scopes work with zero config here too — add a block only to override
one, or to define a wholly new name for `[profiles.<name>]` to select.

| Key | Description |
|---|---|
| `source` | `"scrollback"`, `"viewport"`, or `"tab"`. Unrecognized/absent falls back to `"scrollback"`. |
| `lines` | Max lines to scan. `0` or absent means unbounded. |
| `disable` | Type tags/custom pattern names to skip only while this profile is active — merged into the invocation's disable list, not a replacement for it. Ignored entirely if a keybind's `[profiles.<name>].patterns` allowlist is also set (allowlist mode overrides every disable source, global and per-profile alike). |

**Built-in definitions:**

| Name | `source` | `lines` |
|---|---|---|
| `quick` | scrollback | 150 |
| `deep` | scrollback | 1500 |
| `viewport` | viewport | unbounded |
| `full` | scrollback | unbounded |
| `tab-scan` | tab | 150 |

**Cycling grabbers live:** press `Ctrl-G` in the picker to re-capture
and re-extract with the next grab profile, in the order above followed
by any wholly custom names you've defined here (sorted alphabetically)
- e.g. quick → deep → viewport → full → tab-scan → jira-deep → quick.
The active grabber shows in its own box to the right of the query
field, e.g. `grab:quick (150)` - the resolved line cap is shown for
every source except `viewport` (a screen capture, not a line count);
an unbounded profile (`full`, or any custom one with no `lines` set)
shows `(unbounded)` instead of a number. A failed re-capture (e.g. a
closed pane) leaves the current matches and displayed name untouched
rather than clearing the list.

Unlike the original plugin (where defining even one profile under its
`grab { profiles { } }` block replaces *all four* built-in defaults at
once), a block here overrides or adds by name only — built-ins for
names you don't touch survive. Fields you omit within a block you do
define fall back to that name's built-in value, if one exists (e.g.
`[grab_profiles.deep]` with just `lines = 3000` keeps `deep`'s built-in
`scrollback` source). An unrecognized name with no block at all falls
back to `quick`, matching the original's "typos fall back to the first
defined profile" behavior.

---

## `[colors]` — full UI palette override

```toml
[colors]
cursor_bg = "#7aa2f7"   # Tokyo Night blue
cursor_fg = "#1a1b26"   # Tokyo Night background
```

Every key is optional — omit the block entirely (or any individual
key) to keep the built-in ANSI-palette default for that slot.

### Color value format

| Format | Example | Notes |
|---|---|---|
| ANSI name | `"dark_gray"` | `black`, `dark_gray`, `gray`, `white`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `light_red`, `light_green`, `light_yellow`, `light_blue`, `light_magenta`, `light_cyan` |
| Hex | `"#rrggbb"` | Six-digit lowercase hex |
| RGB | `"rgb(r,g,b)"` | Decimal 0–255 per channel |

An unrecognized value falls back to that slot's built-in default
rather than erroring.

### UI chrome slots

| Key | Default | Used for |
|---|---|---|
| `muted` | `"dark_gray"` | Gutters, hints, secondary text, empty-state messages |
| `accent` | `"cyan"` | Selected-item `*` gutter |
| `cursor_bg` | `"blue"` | List cursor row background |
| `cursor_fg` | `"black"` | List cursor row foreground — must contrast `cursor_bg` |
| `highlight` | `"yellow"` | Fuzzy-match character highlights, config-missing banner border, status messages |
| `error` | `"light_red"` | Rejection/failure status messages (e.g. a refused batch action) |
| `fallback_type` | `"gray"` | Color for a custom pattern with no dedicated `type_*` slot |

### Type color slots

Each slot controls the `[tag]` pill in the list.

| Key | Default | Type tag |
|---|---|---|
| `type_url` | `"blue"` | `url` |
| `type_file` | `"green"` | `file` |
| `type_diag` | `"light_red"` | `diag` |
| `type_git` | `"yellow"` | `git` |
| `type_sha` | `"yellow"` | `sha` |
| `type_ipv4` | `"cyan"` | `ipv4` |
| `type_ipv6` | `"cyan"` | `ipv6` |
| `type_uuid` | `"magenta"` | `uuid` |
| `type_quoted` | `"gray"` | `quote` |
| `type_command` | `"light_magenta"` | `cmd` |
| `type_secret` | `"light_red"` | `secret` |

### Theme presets

Five complete presets — **Catppuccin Mocha**, **Catppuccin Macchiato**,
**Catppuccin Latte** (light), **Tokyo Night**, and **Gruvbox Dark** —
are included as commented blocks in
[`config.example.toml`](../config.example.toml); uncomment one at a
time. Not yet ported: preview-pane match-line highlighting doesn't
exist yet (Phase 9), so `highlight`'s preview-related use from the
original doesn't apply here yet either.

---

## `[types.<tag>]` — per-type verb allow-list and default-verb override

```toml
[types.url]
actions = ["open", "copy-raw"]
default = "copy-raw"
```

`<tag>` is a built-in type tag (see the list under `[patterns]` above)
or a custom pattern's configured `name`.

| Key | Description |
|---|---|
| `actions` | Verbs available for this type, replacing the built-in allow-list entirely. Unrecognized verb names are silently dropped. |
| `default` | Verb fired by `Enter`. Falls back to the built-in default if unset, or if it names a verb not in the (possibly overridden) `actions` list — in that case the first allowed verb wins instead. |

**Verb names** (as used in `actions`/`default`, and distinct from the
picker's own key labels): `copy-raw`, `copy-display`, `insert`,
`insert-display`, `open`, `edit`, `reveal`, `json`.

**Hardcoded exceptions, preserved regardless of this block:**
`copy-raw` and `json` are always allowed for every type; `open`,
`edit`, and `reveal` are always denied for `secret`.

**Interaction with `[limits]`:** a verb whose `[limits]` cap is set to
`0` is dropped from every type's allow-list, even one that explicitly
lists it here.

---

## `[actions.<tag>]` — command templates for open/edit/reveal

```toml
[actions.file]
edit = "hx {file}:{line}"

[actions.default]
open = "open {raw}"
```

`<tag>` is a built-in type tag, a custom pattern's configured `name`,
or the literal `"default"` — consulted for any type with no
tag-specific override for that verb.

| Key | Description |
|---|---|
| `open` | Command template run in place of the built-in `open`/`xdg-open` invocation. |
| `edit` | Command template run in place of the built-in `$EDITOR [+line] file` invocation (also used per-file in a multi-target edit batch). |
| `reveal` | Command template run in place of the built-in `open -R`/`xdg-open <parent>` invocation. |

Each template is run through `sh -c` after variable substitution.

### Template variables

| Variable | Value |
|---|---|
| `{editor}` | `$EDITOR`, falling back to `vi` |
| `{file}` | The match's `file` field, falling back to `raw` |
| `{line}` | The match's `line` field, empty if absent |
| `{url}` | The match's `url` field, falling back to `raw` |
| `{match}` | The match's `match` field (custom patterns), falling back to `raw` |
| `{raw}` | The match's raw value |
| `{display}` | The match's display value |
| `{type}` | The match's effective `#tag` |
| `{context}` | The full source line the match came from |
| `{0}`..`{N}` | Numbered capture groups, for custom patterns |

Unknown `{name}` tokens are left literal. When `{line}` resolves
empty, one trailing separator character (`:`, `+`, or a space)
immediately preceding it in the template is stripped too, so
`"hx {file}:{line}"` degrades to `"hx src/main.rs"` rather than
`"hx src/main.rs:"`.

---

## `[limits]` — per-verb multi-target dispatch caps

```toml
[limits]
insert = 1
```

| Key | Default | Caps |
|---|---|---|
| `copy` | 100 | `copy-raw` and `copy-display` batches together |
| `insert` | 5 | `insert` and `insert-display` batches together |
| `open` | 10 | `open` |
| `edit` | 5 | `edit` |
| `reveal` | 10 | `reveal` |
| `json` | 100 | `json` |

Omitting a key keeps its built-in default. Setting a key to `0`
disables that verb entirely, for every type — the picker refuses it
the same way it refuses a type-mismatched verb, rather than treating it
as "cap exceeded."
