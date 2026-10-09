# Built-in match types

`herdr-zextract` scans scrollback for eleven built-in types (the
original ships ten — this port adds `git`, see below). Each type has a
short `#tag` used in query filters, `[types.<tag>]` overrides, and
`[actions.<tag>]` command templates — see
[`doc/config-reference.md`](config-reference.md) for that config
surface, and [`doc/patterns.md`](patterns.md) for exactly how each
type is detected. This doc is the quick-reference table: verbs,
default action, template fields, color.

**Universal verbs, every type:** `copy-raw` and `json` (export) are
always allowed regardless of `[types.<tag>].actions` — the hardcoded
exceptions `actions.rs` layers on top of any config override. `reveal`
is real, dispatchable machinery but isn't in *any* type's default
allow-list; opt it in per type via `[types.<tag>].actions`.

---

## `url` — URLs and URIs

**Tag:** `#url` · **Default action:** open in browser
**Available verbs:** open, copy-raw, insert, json

| Field | Value |
|---|---|
| `{url}` | Full URL |
| `{scheme}` | e.g. `https` |
| `{host}` | e.g. `github.com` |

---

## `file` — File paths

**Tag:** `#file` · **Default action:** insert into source pane
**Available verbs:** edit, copy-raw, insert, json

| Field | Value |
|---|---|
| `{file}` | Path without line/col suffix |
| `{line}` | Line number, or empty |
| `{col}` | Column number, or empty |
| `{dir}` | Parent directory |
| `{basename}` | Filename with extension |
| `{ext}` | Extension without the dot |

> The original defaults `file` to `edit`. This port's default is
> `insert`, matching `actions.rs::static_default_verb`'s *actual*
> behavior (verified against source, not the original's own doc, which
> disagrees with its own code here). Set `[types.file].default =
> "edit"` if you want the original's documented behavior.

---

## `diag` — Compiler/linter diagnostics

**Tag:** `#diag` · **Default action:** open in editor at the matched line
**Available verbs:** edit, copy-raw, insert, json

Same template fields as `file`: `{file}`, `{line}`, `{col}`, `{dir}`,
`{basename}`, `{ext}` (`{col}`/`{message}` empty for the Python
traceback form).

---

## `git` — Git log entries

**Tag:** `#git` · **Default action:** insert into source pane
**Available verbs:** insert, copy-raw, json

**Not in the original's type set** — added so a `git log` line's
commit subject travels with its hash in the picker list, instead of
showing as a bare hash indistinguishable from any other `sha` match.

| Field | Value |
|---|---|
| `{sha}` | The commit hash (also `raw`) |
| `{subject}` | Commit subject line, when found |

`display` in the picker list is `<hash> <subject>`; `raw` (what
copy/insert/export act on) is the hash alone.

---

## `sha` — Bare Git commit hashes

**Tag:** `#sha` · **Default action:** insert into source pane
**Available verbs:** copy-raw, insert, json

`{sha}` — the hash (also `raw`).

---

## `ipv4` — IPv4 addresses

**Tag:** `#ipv4` · **Default action:** insert into source pane
**Available verbs:** copy-raw, insert, json

`{ip}`, `{port}` (empty when absent).

---

## `ipv6` — IPv6 addresses

**Tag:** `#ipv6` · **Default action:** insert into source pane
**Available verbs:** copy-raw, insert, json

`{ip}`, `{port}` (empty when absent).

> **On by default** in this port, unlike the original (off by default,
> opt-in). Disable with `[patterns] disable = ["ipv6"]` if unwanted.

---

## `uuid` — UUIDs

**Tag:** `#uuid` · **Default action:** insert into source pane
**Available verbs:** copy-raw, insert, json

`{uuid}` — the UUID (also `raw`).

---

## `quote` — Quoted strings

**Tag:** `#quote` · **Default action:** insert into source pane
**Available verbs:** copy-raw, copy-display, insert, insert-display,
json — the only type with both display-variant verbs allowed by
default

| Field | Value |
|---|---|
| `{content}` | The unquoted inner string (also `display`) |
| `{quote}` | Which delimiter was used (`"`, `'`, `` ` ``) |

`raw` includes the surrounding quotes; `display`/`{content}` doesn't.

---

## `cmd` — Shell commands

**Tag:** `#cmd` · **Default action:** insert into source pane
**Available verbs:** insert, copy-raw, json

| Field | Value |
|---|---|
| `{match}` | The full command text (also `raw`) |
| `{hint}` | Inline `# comment` text following the command, if present |

---

## `secret` — Credentials and tokens

**Tag:** `#secret` · **Default action:** insert into source pane
**Available verbs:** copy-raw, insert, json — `open`/`edit`/`reveal`
are **hardcoded denied**, unconditionally, even via
`[types.secret].actions`

| Field | Value |
|---|---|
| `{secret}` | The token (also `raw`) |
| `{secret_format}` | Detected format, e.g. `"jwt"`, `"github"`, `"entropy"` |

---

## Cross-type dedup

When two pattern types match the same text, the higher-priority type
wins. Priority order (highest first):

```
url > diag > file > uuid > git > sha > ipv4 > ipv6 > cmd > secret > quote
```

So a Git-log line's hash is classified `git` even though the bare hash
also matches `sha`, and a quoted URL (`"https://example.com"`) is
classified `url` even though it also matches `quote`.

---

## Type color reference

Every slot is overridable via `[colors]` — see
[`doc/config-reference.md`](config-reference.md#colors--full-ui-palette-override).
Defaults:

| Type | Color |
|---|---|
| `url` | Blue |
| `file` | Green |
| `diag` | Light red |
| `git` | Yellow |
| `sha` | Yellow |
| `ipv4` | Cyan |
| `ipv6` | Cyan |
| `uuid` | Magenta |
| `quote` | Gray |
| `cmd` | Light magenta |
| `secret` | Light red |
| Custom patterns | `fallback_type` (Gray) unless overridden |
