# Keybinding reference

`herdr-zextract` never binds its own keys — Herdr's own
`~/.config/herdr/config.toml` owns all keybindings, via
`[[keys.command]]` entries with `type = "plugin_action"`. This doc
covers the actions this plugin ships, how to bind them, and how to
configure a combination the shipped four don't cover — without editing
this repo's manifest.

## Shipped actions

`herdr-plugin.toml` declares fourteen `[[actions]]`, each a thin
launcher that opens the real interactive popup (`herdr plugin pane
open`) with `--env ZEXTRACT_PROFILE=<name>` selecting a named profile.
The profile's actual grab scope / pattern allowlist / query pre-fill
live in **your own** `config.toml`, not here — see
[`doc/config-reference.md`](config-reference.md#profilesname--per-keybind-grab-scope-pattern-allowlist-query-pre-fill).

Four have built-in defaults, so they work with zero config:

| Action id          | Profile name | Grab scope    | Pattern filter        | Preview |
| ------------------- | ------------ | -------------- | ---------------------- | ------- |
| `zextract-open`     | `open`       | current pane   | all types               | off (`[ui].preview` default) |
| `zextract-tab`      | `tab`        | whole tab       | all types               | off (`[ui].preview` default) |
| `zextract-url`      | `url`        | current pane   | `url`, `ipv4`, `ipv6`   | on |
| `zextract-url-tab`  | `url-tab`    | whole tab       | `url`, `ipv4`, `ipv6`   | on |

Loosely mirrors the original `zellij-zextract` plugin's current-pane/
whole-tab, all-types/URL-only split (lowercase = current pane,
uppercase = whole tab; `u`/`U` = URLs/IPs only) — the exact letters
below are just the example binding, pick whatever's free on your setup.

The other ten — `zextract-custom0` through `zextract-custom9` — select
profile names `custom0`..`custom9`, which have **no** built-in default.
Bind one and define its `[profiles.customN]` block in your own
`config.toml` to give it real behavior; bound-but-undefined ones just
degrade to plain defaults (`quick` grab, no pattern restriction)
instead of failing.

## Binding a shipped action

Add one `[[keys.command]]` entry per action to
`~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "zextract-open"

[[keys.command]]
key = "prefix+shift+f"
type = "plugin_action"
command = "zextract-tab"

[[keys.command]]
key = "prefix+u"
type = "plugin_action"
command = "zextract-url"

[[keys.command]]
key = "prefix+shift+u"
type = "plugin_action"
command = "zextract-url-tab"
```

Validate before reloading:

```sh
herdr config check
herdr server reload-config
```

If more than one installed plugin happens to define an action with the
same id, disambiguate with `command = "herdr-zextract:zextract-open"`.

## Configuring a combination the shipped four don't cover

There's no built-in "URLs only, whole tab, pre-filtered to just IPv6"
binding, for instance. You don't need to touch `herdr-plugin.toml` for
this — pick one of the ten free `zextract-customN` actions, bind it,
and define its profile in your own `config.toml`:

```toml
[[keys.command]]
key = "prefix+shift+u"    # any key you like
type = "plugin_action"
command = "zextract-custom0"
```

```toml
# in config.toml
[profiles.custom0]
grab = "tab-scan"
patterns = ["ipv6"]
```

```sh
herdr config check
herdr server reload-config
```

No relink, no rebuild, no manifest edit — the profile lives entirely
in your own config. Full schema for `[profiles.<name>]`:
[`doc/config-reference.md`](config-reference.md).

## Adding an eleventh action

If all ten `customN` slots are taken, or you want a distinct action id
or title (e.g. for a nicer name in `herdr plugin action list`), add
your own `[[actions]]` entry to `herdr-plugin.toml`, modeled on the
existing ones — it just needs a unique id and a
`--env ZEXTRACT_PROFILE=<name>` matching a profile you define in your
own `config.toml`:

```toml
[[actions]]
id = "zextract-jira"
title = "zextract: Jira tickets only"
command = ["herdr", "plugin", "pane", "open", "--plugin", "herdr-zextract", "--entrypoint", "zextract", "--env", "ZEXTRACT_PROFILE=jira"]
```

Then re-link so Herdr picks up the new action id, and bind a key to it
exactly as above:

```sh
herdr plugin link .        # from this repo's root
herdr config check
herdr server reload-config
```

**Platform gap — no per-launch popup title:** the original plugin's
`configuration.popupTitle` lets a keybind override the popup's title
at launch time. Herdr has no equivalent — every `zextract-*` action
above shares the one `[[panes]]` entry (`id = "zextract"`, `title =
"zextract"`) via `--entrypoint zextract`, and a pane's title is fixed
by its `[[panes]].title` at manifest-link time, not by the launching
action's own `title` (that's only the label shown in `herdr plugin
action list`) or by `ZEXTRACT_PROFILE`/`config.toml`. Getting a
distinct popup title per profile means defining a second `[[panes]]`
block with its own `id`/`title` and pointing a new action's
`--entrypoint` at that id instead — not just adding `[[actions]]` as
shown above.
