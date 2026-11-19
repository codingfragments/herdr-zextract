# Use cases

Worked walkthroughs showing `herdr-zextract` in real scenarios. See
[`doc/keybinding.md`](keybinding.md) for how actions/keybinds fit
together, [`doc/config-reference.md`](config-reference.md) for the
full config schema, and [`doc/patterns.md`](patterns.md)/
[`doc/types.md`](types.md) for what each `#tag` actually matches.

---

## Open a URL from build output

You're watching a dev server's output and spot a couple of localhost
URLs in the logs:

```
     Running server on http://localhost:3000
     API available at http://127.0.0.1:8080/api/v1
```

**Flow:**
1. Press your `zextract-url` keybind (`u` in the example bindings).
2. The two localhost URLs appear at the top (most recent first) — the
   preview pane is already open by default for this profile, showing
   the surrounding log lines.
3. Navigate to `http://localhost:3000`, press `Enter` → opens in your
   browser.

This already works with zero config — `url`/`url-tab` are two of the
four built-in profiles, restricting extraction to `url`/`ipv4`/`ipv6`
and starting with the preview open. See
[`doc/keybinding.md`'s shipped actions table](keybinding.md#shipped-actions).

---

## Jump to a diagnostic in the editor

A compiler error points to a file and line:

```
error[E0382]: borrow of moved value
  --> src/main.rs:142:8
```

**Flow:**
1. Open the picker.
2. Type `#diag` to show only diagnostics.
3. Navigate to `src/main.rs:142:8`, press `Enter` → spawns `$EDITOR` at
   that file/line directly (Herdr plugins get a real PTY, unlike the
   original's WASM sandbox, which could only *type* the editor command
   into the source pane for you to run yourself).

**Config for a specific editor**, in your `config.toml`:

```toml
[actions.diag]
edit = "hx {file}:{line}"
```

```toml
[actions.diag]
edit = "code -g {file}:{line}"
```

`{line}`'s trailing separator is stripped automatically when a match
has no line number, so the same template degrades gracefully for a
plain `file` match with no `:line` suffix.

---

## Insert a command back to the prompt for review

You see a long command in your scrollback and want to re-run it with
modifications:

```
kubectl get pods -n production --field-selector=status.phase=Running
```

**Flow:**
1. Open the picker, type `#cmd` to filter to commands.
2. Tab into List mode, navigate to the kubectl line, press `i`
   (insert).
3. The command lands at your source pane's prompt — edit as needed,
   then press Enter there to run it.

---

## Export multiple file paths as JSON for scripting

A build failure lists several affected files and you want to pipe them
to a script:

```
error in src/auth/login.rs:42
error in src/auth/session.rs:17
error in src/api/routes.rs:88
```

**Flow:**
1. Open the picker, type `#file`.
2. Press `Space` on each file match to multi-select.
3. Press `Ctrl-J` (or `J` in List mode) — the selection is printed to
   stdout as a JSON array, and the popup closes immediately after.

**JSON output shape** (every extracted field, plus `type`/`raw`/
`display`/`context`):

```json
[
  {"type":"file","raw":"src/auth/login.rs:42","display":"src/auth/login.rs:42","context":"error in src/auth/login.rs:42","file":"src/auth/login.rs","line":"42","col":"","dir":"src/auth","basename":"login.rs","ext":"rs"}
]
```

> **Known limitation:** since the popup closes as soon as the action
> finishes (no more "press Enter to close" pause), the JSON prints to
> a terminal pane that's about to disappear — there's no automatic way
> to actually capture that output today. If you need this for
> scripting, redirect it via `herdr plugin log` inspection, or ask for
> a follow-up that copies the JSON to the clipboard instead of
> printing it (matching how `copy`/`copy-display` already behave).

---

## Wire a dedicated custom-pattern keybind

Your team uses ticket references everywhere in commit messages and PR
descriptions. You want a keybind to instantly show all ticket refs in
scrollback, expanded to clickable URLs.

**Step 1** — add the pattern and a profile to your `config.toml`:

```toml
[[patterns.custom]]
name = "jira"
regex = "([A-Z]+)-([0-9]+)"
type = "url"
template = "https://your-company.atlassian.net/browse/{1}-{2}"

[profiles.custom0]
grab = "deep"            # search 1500 lines back
patterns = ["jira"]      # only run the jira pattern - skip secret/sha/etc.
type_filter = ["jira"]   # pre-filter list to jira matches on open
```

**Step 2** — bind a key to the matching `zextract-custom0` action
(already declared in `herdr-plugin.toml` with no edits needed) in your
own `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+shift+j"
type = "plugin_action"
command = "zextract-custom0"
```

```sh
herdr config check
herdr server reload-config
```

**Result:** the keybind opens a picker pre-filtered to `[jira]` matches
like `https://your-company.atlassian.net/browse/PROJ-123`. Press
`Enter` → opens in your browser.

---

## Extract a context-anchored value

Your logs prefix ticket refs with a label, and you only want the ID,
not the whole prefix:

```
New ticket : ST-154R    assigned to alice
Blocked by : BACKEND-42 waiting on API contract
```

Use a context-prefix pattern with a capture group:

```toml
[[patterns.custom]]
name = "ticket"
regex = "(?:New ticket|Blocked by) : ([A-Z]+-[0-9]+[A-Z]*)"
type = "url"
template = "https://your-company.atlassian.net/browse/{1}"
```

The prefix (`New ticket : ` / `Blocked by : `) is required to trigger
the match, but only the captured group (`ST-154R`) becomes `{1}` in
the template. The picker shows
`[ticket]  https://...atlassian.net/browse/ST-154R`.

---

## Widen the search, or preview a match's context, without relaunching

The picker shows "No matches in pane scrollback" — your terminal only
keeps a short viewport buffer for the `quick` profile (150 lines by
default).

**Flow:**
1. Press `Ctrl-G` to cycle to the next grab profile — `deep` (1500
   lines), then `viewport`, then `full` (entire scrollback, no cap),
   then back around through any custom `[grab_profiles.<name>]` you've
   defined. The grab label to the right of the query field updates
   with each press, e.g. `grab:deep (1500)`, and the list refreshes in
   place.
2. Press `p` (List mode) or `Ctrl-P` (either mode) to toggle the
   preview split — shows ±3 lines of context around the highlighted
   match, with its own source line and the exact extracted text both
   picked out in color. Useful for disambiguating which of several
   similar-looking matches you actually want before committing to an
   action.

No relaunch needed for either — both act on the picker that's already
open.
