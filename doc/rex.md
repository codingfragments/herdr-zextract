# Running zextract under Rex

The same `herdr-zextract` binary works under
[Rex](https://github.com/codingfragments/herdr-zextract) (superlogical's
terminal multiplexer) and [Herdr](https://herdr.dev) — one config, one
release. This doc covers the Rex side.

## How it works

```
ctrl+s>f ──► ~/.config/rex/plugins/zextract.lua (the launcher)
              │ finds the focused pane of the active window
              │ opens a floating layer (80%×80%) over it
              ▼
            herdr-zextract (ZEXTRACT_HOST=rex)
              │ rex capture …      read the source pane's scrollback
              │ rex session inspect … list the tab's panes (tab profile)
              │ rex send …         insert a match back into the pane
```

The launcher reads the focused pane **before** the layer takes focus
(inside the popup, `REX_BLOCK` names the popup itself) and passes it as
`ZEXTRACT_SOURCE_BLOCK`. `ZEXTRACT_HOST=rex` is explicit, so a herdr
running inside a Rex terminal (or the reverse) is never detected as the
wrong host.

## Install

**1. The binary** (Rex side; under herdr use `herdr plugin install`
instead):

```sh
mise use -g github:codingfragments/herdr-zextract   # pin a version in your dotfiles
```

or build from source:
`cargo install --git https://github.com/codingfragments/herdr-zextract --tag v0.7.0`.

**2. The launcher** — copy [`rex/zextract.lua`](../rex/zextract.lua) to
`~/.config/rex/plugins/zextract.lua` and add to `init.lua`:

```lua
require("plugins.zextract")
```

The launcher uses `lib/env.lua` (login PATH, rex CLI path) when your
config has it and falls back to inline defaults otherwise; a binary
installed with mise is only found with that file, see
[Environment](#environment-the-rex-cli-and-your-login-path) below.
Reload your config (`ctrl+s>shift+r`).

**3. Config** — one file serves both hosts:
`~/.config/zextract/config.toml` (see
[`config-reference.md`](config-reference.md)). A `[host.rex]` table can
override `[ui]`/`[actions]` for Rex only. The herdr location
(`herdr plugin config-dir herdr-zextract`) is still read as a fallback
when running under herdr.

## Environment: the `rex` CLI and your login PATH

The launcher needs two things from your Rex config, because the Rex
server is started by the macOS app with a minimal `PATH`
(`/usr/bin:/bin:/usr/sbin:/sbin`, no Homebrew, no mise shims):

| It needs | Without it |
|---|---|
| the **`rex` CLI's absolute path** | the popup opens, but `herdr-zextract` cannot call back into Rex |
| a **login `PATH`** that contains `herdr-zextract` | the popup says `zextract: herdr-zextract not found on this host` |

On a standard macOS install the CLI is found with no setup (the launcher
falls back to the app bundle under your own `$HOME`). Its fallback
`PATH`, though, has **no mise shims and no `~/.local/bin`**, so a binary
installed with `mise use -g` is **not found until you add a
`~/.config/rex/lib/env.lua`**. One file serves every Rex launcher:

```sh
mkdir -p ~/.config/rex/lib
curl -fsSL https://raw.githubusercontent.com/codingfragments/muxhost/main/examples/rex/lib/env.lua \
  -o ~/.config/rex/lib/env.lua
```

That sample finds the CLI and asks your login shell (fish, zsh, bash, …)
for its `PATH`, with nothing tied to an account or a home directory. To
pin values by hand instead, to check what it resolves to, or to
troubleshoot, see the muxhost guide:
[The Rex environment for launchers](https://github.com/codingfragments/muxhost/blob/main/doc/rex-environment.md).
Reload your config afterwards (`ctrl+s>shift+r`).

## Bindings

| Key | Profile | Grabs |
|---|---|---|
| `ctrl+s>f` | `open` | the focused pane, all types |
| `ctrl+s>shift+f` | `tab` | every pane of the tab |
| `ctrl+s>u` | `url` | the focused pane, URLs/IPs only |
| `ctrl+s>shift+u` | `url-tab` | the whole tab, URLs/IPs only |

Any `[profiles.<name>]` you define can be bound without packaging:
`rex.bind("ctrl+s>z", "zextract", { profile = "custom0" })`.

In the picker everything is the same as under herdr: fuzzy filter,
`Tab` mode toggle, `Ctrl-G` grab cycle, `Enter` default action,
`y/o/e/p` copy/open/edit/insert, `Esc` cancel.

## Remote hosts and headless machines

The layer runs on the host that runs the terminal, so on a remote Rex
session the binary must be installed **there** (mise on that host). The
launcher's preflight names the host and the install command when the
binary is missing or too old. Copy/open/edit act on that host.
Clipboard: the system clipboard when there is one, else an OSC 52
sequence that the terminal forwards.

## Limits under Rex

- Grab sources: scrollback with a line cap (`quick`/`deep`/`full`) and
  viewport map to `rex capture`; the `tab-scan` profile lists the tiled
  panes of the source window. No popup/own pane ever appears.
- `insert` types into the pane the launcher was invoked from, like
  under herdr.
- No herdr plugin actions (`plugin_action` bindings) — Rex has no plugin
  system; the bindings above replace them.
