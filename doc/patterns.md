# Built-in pattern reference

`herdr-zextract` scans every scrollback line against all enabled patterns
and presents the combined results in the fuzzy picker. Each built-in
pattern is described below: what it matches, how it detects matches,
and what `raw` value (used for copy/insert/dedup) it produces.

Ported from the original `zellij-zextract` plugin's pattern set
(`crates/zextract/src/pattern/*.rs`), ~~near-verbatim~~ with a few
deliberate differences noted per section below — this doc describes
what this Rust port's regexes actually do, verified against
`src/matcher/*.rs`, not the original's behavior.

All patterns are enabled by default. Disable any of them (or a custom
pattern) via `[patterns].disable` in `config.toml` — see
[`doc/config-reference.md`](config-reference.md). Verb allow-lists,
defaults, and dispatch caps per type are in
[`doc/config-reference.md`'s `[types]`/`[actions]`/`[limits]`
sections](config-reference.md#typestag--per-type-verb-allow-list-and-default-verb-override);
this doc only covers *detection*.

---

## `url` — URLs

**Tag:** `#url`
**Default action:** open in browser

Matches `(https?|ftp|file|git|ssh)://` followed by one or more
non-whitespace, non-bracket characters. **Narrower than the original**,
which also recognizes `ftps`, `svn`, `mailto`, `irc`, `ircs`, `slack`,
and `vscode` schemes — those aren't ported.

Trailing punctuation unlikely to be part of the URL (`.`, `,`, `;`,
`:`, `!`, `?`, `)`, `]`, `}`, `>`, `"`, `'`) is stripped from the right
end.

Captures `{url}` (the whole match), `{scheme}`, `{host}` for command
templates.

**Examples matched:**

```
https://github.com/owner/repo/pull/42
http://localhost:3000/api/v1/users
git://git.example.com/myrepo.git
file:///home/user/.config/herdr/config.toml
```

---

## `file` — File paths

**Tag:** `#file`
**Default action:** insert into source pane (not "open in editor" like
the original — see the module doc comment note below)

Matches absolute (`/…`), home-relative (`~/…`), explicit-relative
(`./…`, `../…`), and relative-with-a-slash (`src/main.rs`) paths. A
bare filename with no separator (`Cargo.toml`) does **not** match —
add a `./` prefix to force one. An optional `:line` or `:line:col`
suffix is captured into `{line}`/`{col}` and stripped from the
matched path.

Guards against false positives:
- The character immediately before the match must be a plausible
  word-start (line start, whitespace, or one of `([{<"'`=,;`) — rejects
  `/foo` fragments glued onto a URL's `://` or run into another word
  like `runfile/foo`.
- Matches shorter than 3 characters are dropped.
- Pure-numeric "paths" (`2.5`, `42.0`) are excluded.
- Numeric ratios (`1/1`, `42/100`) are excluded — common in progress
  indicators and fractions, never real paths.

Captures `{file}` (path without line/col), `{line}`, `{col}`, `{dir}`
(parent directory), `{basename}`, `{ext}`.

**Examples matched:**

```
/home/user/.config/herdr/config.toml
src/main.rs:42:8
./scripts/install.sh
crates/zextract/src/pattern/command.rs
```

> **Note — default verb differs from the original:** the original's
> `file` type defaults to "open in editor." This port's
> `static_default_verb` puts `file` in the same bucket as everything
> except `url`/`diag` (Insert), matching the *actual* upstream source
> code rather than its own stale doc (`doc/types.md` there claims
> `file` defaults to edit — the code doesn't). Override with
> `[types.file].default = "edit"` if you want the original's
> documented-but-not-actually-shipped behavior.

---

## `diag` — Compiler/linter diagnostics

**Tag:** `#diag`
**Default action:** open in editor at the matched line

Two forms:

- **Colon form:** `path:line:col` — the `rustc`/`gcc`/`clang`/`eslint`
  style. The path must contain a `/` or a `.ext` filename component.
- **Python traceback:** `File "path", line N`.

Guarded the same way as `file` (word-start check on the preceding
byte). Captures `{file}`, `{line}`, `{col}` (empty for the Python
form), `{message}` (always empty currently — filling it in would need
inspecting surrounding context lines, not done), `{dir}`, `{basename}`,
`{ext}`.

**Examples matched:**

```
src/main.rs:42:8
crates/zextract/src/extract.rs:140:1
  File "/home/user/app.py", line 23, in main
```

---

## `git` — Git log entries

**Tag:** `#git`
**Default action:** insert into source pane

**Not in the original's pattern set** — this is a dedicated type this
port adds on top of the bare `sha` pattern, specifically for `git log`
output, so a commit's subject line travels with its hash instead of
being picked up as an undifferentiated `sha` match.

Recognizes:
- `git log --oneline` (with or without `--graph`): a 7–40 char hex hash
  followed by the commit subject.
- `git log`'s full-format `commit <hash>` header line, looking ahead up
  to 7 lines for the commit message's first 4-space-indented line.

Handles `--color` output (ANSI codes stripped before matching) and
`bat`/`less -N` pager line-number prefixes (`  123  │ `). `raw` is just
the hash (what copy/insert/dedup act on); `display` is `<hash>
<subject>` for the picker list. Captures `{sha}`, `{subject}`.

**Examples matched:**

```
78bef8d Fix flaky test in matcher module
commit a1b2c3d4e5f6789012345678901234567890abcd
    Add config-driven preview split
```

---

## `sha` — Bare Git commit SHAs

**Tag:** `#sha`
**Default action:** insert into source pane

Matches 7–40 character hexadecimal strings at word boundaries, with at
least one `a`-`f`/`A`-`F` letter required (rejects pure-numeric runs
like line numbers or timestamps). ANSI escape codes are stripped first
so `git log --color` output works. A hash already captured by the
`git` pattern above isn't picked up separately by this one (`git`
ranks ahead of `sha` in the dedup priority order).

**Examples matched:**

```
78bef8d
d7d8438f19a2bc3
a1b2c3d4e5f6789012345678901234567890abcd
```

---

## `ipv4` — IPv4 addresses

**Tag:** `#ipv4`
**Default action:** insert into source pane

Matches dotted-quad notation with each octet validated 0–255 in code
(the regex itself is permissive; invalid octets are rejected after
matching). An optional `:port` suffix (validated 1–65535) is included.

Captures `{ip}`, `{port}` (empty when absent).

**Examples matched:**

```
192.168.1.1
10.0.0.1:8080
127.0.0.1
```

---

## `ipv6` — IPv6 addresses

**Tag:** `#ipv6`
**Default action:** insert into source pane
**Default state: on** — unlike the original, which ships this
off-by-default (`types { ipv6 { ... } }` opt-in). Disable it yourself
with `[patterns] disable = ["ipv6"]` if it's too noisy in your
scrollback.

Matches full 8-group form, `::`-compressed form, and bracketed form
with a port (`[::1]:8080`). Validated in code after a permissive regex
match: no more than one `::`, each group ≤ 4 hex chars, exactly 8
groups when `::` isn't present.

Captures `{ip}`, `{port}` (empty when absent).

**Examples matched:**

```
2001:db8::1
::1
[fe80::1]:443
```

---

## `uuid` — UUIDs

**Tag:** `#uuid`
**Default action:** insert into source pane

Matches any version UUID in the standard 8-4-4-4-12 hex-group form,
case-insensitive. Captures `{uuid}`.

**Examples matched:**

```
123e4567-e89b-12d3-a456-426614174000
A987FBC9-4BED-3078-CF07-9141BA07C9F3
```

---

## `quote` — Quoted strings

**Tag:** `#quote`
**Default action:** insert into source pane (also allows copy/
copy-display/insert-display — the only type with all four)

Matches text enclosed in a matching pair of `"`, `'`, or `` ` `` on the
same line. Empty and single-character contents are excluded — too
noisy. Doesn't handle escaped quotes inside the content; capture stops
at the first matching delimiter. The quotes themselves aren't part of
`raw`/`display` — only the content between them.

Captures `{content}` (the inner string), `{quote}` (which delimiter
was used).

**Examples matched:**

```
"hello world"      →  hello world
'~/.config'        →  ~/.config
`some command`     →  some command
```

---

## `secret` — Secrets and tokens

**Tag:** `#secret`
**Default action:** insert into source pane (`open`/`edit`/`reveal`
are always denied, regardless of `[types.secret]` config)

Two-tier detection, curated formats always winning over the entropy
fallback for the same span:

**Tier 1 — curated format regexes** (high precision):

| Format | Shape |
|---|---|
| `jwt` | `eyJ` + three base64url segments separated by `.` |
| `aws` | `AKIA`/`ASIA` + 16 uppercase-alphanumeric chars |
| `github` | `gh[pousr]_` + 36+ alphanumeric chars |
| `github_pat` | `github_pat_` + 82 alphanumeric/underscore chars |
| `gitlab` | `g(lpat|loas|lptt|lrt|lsoat|lagent)-` + 20+ chars |
| `stripe` | `sk_live_`, `sk_test_`, `pk_live_`, `pk_test_`, `rk_live_`, or `whsec_` + 24+ alphanumeric chars |
| `openai` | `sk-` (optionally `sk-proj-`) + 20+ chars |
| `anthropic` | `sk-ant-(api\|admin)NN-` + 20+ chars |
| `slack` | `xox[abprs]-` + 10+ alphanumeric/hyphen chars |
| `bearer` | `Bearer ` + one or more base64url-ish chars |

**Broader than the original** in a few spots (Stripe's test/restricted
keys and `whsec_` webhook secrets, OpenAI's `sk-proj-` prefix, GitLab's
five additional token-type prefixes beyond plain `glpat-`) — these
were added directly against current vendor token formats rather than
kept at parity with the original's narrower list.

**Tier 2 — entropy fallback** (broader, more false positives; toggle
with `[patterns.secret].entropy_filter`, default on):

Whitespace-delimited tokens 20–200 characters long, using at least 3
of {lowercase, uppercase, digit, `_-+/=.`} character classes, with
Shannon entropy ≥ 3.5 bits/char. Any character outside those classes
disqualifies the token outright.

Captures `{secret}` (the raw token), `{secret_format}` (e.g. `"jwt"`,
`"github"`, `"entropy"`).

---

## `cmd` — Commands

**Tag:** `#cmd`
**Default action:** insert into source pane

Three detection strategies, tried in order per line; a line produces
at most one prompt-anchored or exec-anchored match, plus an additional
flag-anchored match only when neither of the first two fired.

Before any match is emitted it must be ≥5 characters (trimmed) and
contain at least one ASCII letter — rejects pure-numeric/punctuation
noise like a bare `❯` line or a fish right-prompt timestamp
(`18:48:12`) bleeding onto an empty line.

### Strategy 1 — prompt-anchored (always on)

A line starting with a recognized prompt marker (`❯ `, `$ `, `> `,
`% `, `# `) is a command line; the marker is stripped and the rest of
the line is the command.

Trailing-backslash continuation lines are spliced in (up to 10): each
continuation's leading noise (line numbers, diff `+`/`-` markers,
`#`/`>`/`|` comment/quote prefixes, leading whitespace) is stripped
before joining with a single space. A trailing run of 5+ spaces on any
line (prompt or continuation) is treated as a right-prompt
(timestamp/git-status) and trimmed off before splicing. An inline
`" # "` sequence is captured into `{hint}` and excluded from the
command text.

Whole-line comments (`#…`/`//…`) are skipped entirely — never treated
as a command line.

**Examples:**

```
❯ git log --oneline -n 20         →  git log --oneline -n 20
$ cargo build --release           →  cargo build --release
❯ curl -fsSL https://example.com \
    | sudo bash                   →  curl -fsSL https://example.com | sudo bash
```

### Strategy 2 — exec-anchored (always on)

Scans for the leftmost occurrence of a known trigger word at a
command-start position (preceded by line start, whitespace, or a shell
operator/prose-punctuation byte: `` |;&([{`$=><"':, ``). Explicitly
*not* command-start: `.` or `/` immediately before the trigger — that
signals a file extension (`install.sh`) or path component
(`/usr/bin/sh`), not a standalone word. Captures from the trigger to
end of line (before any inline comment). No continuation splicing —
too risky in prose.

**Trigger list** (mirrors the original's, plus `herdr` itself):

| Group | Triggers |
|---|---|
| Package managers | `sudo` `apt` `apt-get` `yum` `dnf` `pacman` `brew` `snap` `pip` `pip3` `pipx` `gem` `cargo` `go` `npm` `yarn` `pnpm` `bun` `uv` `poetry` `conda` `mamba` |
| Fetch | `curl` `wget` `fetch` |
| Shell exec | `sh` `bash` `zsh` `fish` `/bin/sh` `/bin/bash` |
| Build | `make` `cmake` `ninja` `just` `nix` `nix-shell` `nix-build` |
| Editor/pager/IO | `nvim` `vim` `nano` `emacs` `less` `more` `cat` `tee` `xargs` `awk` `sed` `grep` `find` |
| VCS | `git` `hg` `svn` |
| Containers/orchestration/multiplexers | `docker` `podman` `kubectl` `helm` `zellij` `tmux` `herdr` |
| Language runners | `python` `python3` `node` `deno` `ruby` `rustc` `java` `mvn` `gradle` |
| File ops | `tar` `gunzip` `unzip` `chmod` `chown` `ln` `mkdir` `rm` `cp` `mv` `ssh` `scp` `rsync` |

**Examples:**

```
To install run: sudo apt install herdr    →  sudo apt install herdr
[dry-run] zellij --session foo            →  zellij --session foo
Running curl -fsSL https://example.com   →  curl -fsSL https://example.com
```

### Strategy 3 — flag-anchored (opt-in)

**Off by default.** Enable with:

```toml
[patterns.command]
flag_anchored = true
```

Only tried when strategies 1 and 2 both found nothing on a line. Scans
for the leftmost standalone `-x`, `-xyz` (combined short flags), or
`--long-flag` token — "standalone" meaning the byte before the `-` is
whitespace, `(`, `&`, `|`, `;`, or `=` (flags glued inside compound
words like `dry-run` don't count). From the flag, walks backward to
the nearest boundary character (`][}{><:;|&(,'"`), then forward past
whitespace to locate the command word.

Guards: the command word must start with a lowercase ASCII letter and
be ≥2 characters — rejects `The --verbose flag` (uppercase start) and
single-letter noise.

**Examples:**

```
[dry-run] rsync -avz src/ dest/          →  rsync -avz src/ dest/
output: cargo build --release --target   →  cargo build --release --target
[info] ssh -i ~/.ssh/id_ed25519 user@h   →  ssh -i ~/.ssh/id_ed25519 user@host
```

**Known false-positive categories with flag-anchored enabled** (same
trade-off as the original): lowercase prose before a flag
(`"missing argument -v"`) and log lines with a lowercase prefix before
a boundary char (`"note: --edition 2024"`) both produce a spurious
match. Use `#cmd`/`#!cmd` query filters to show or hide command
matches in a session where the noise is too high, rather than leaving
this on globally.

**Not ported from the original:** its further opt-in
extension-anchored and comment-anchored strategies aren't implemented
here.

---

## Deduplication

Every pattern's output feeds into the same two-pass dedup, applied
across all types together:

1. **Same `(type, raw)`** → keep only the latest occurrence (most
   recent in scrollback).
2. **Same `raw` across different types** → keep the type ranked
   highest in `TYPE_PRIORITY` (front of the list wins): `url` > `diag`
   > `file` > `uuid` > `git` > `sha` > `ipv4` > `ipv6` > `cmd` >
   `secret` > `quote`. Ties broken by recency.

So, for example, a URL that also happens to look like a command (e.g.
`git://...`) keeps only its `url` entry.
