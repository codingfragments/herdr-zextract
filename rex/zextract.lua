-- zextract for Rex — the launcher.
--
-- Opens herdr-zextract in a floating layer over the pane you are in:
-- grab scrollback matches from it, act on them (open/edit/copy/insert).
-- The binary does the work; this file only finds the source pane,
-- checks the binary is usable, and starts it with the right context.
--
-- Install: copy this file to ~/.config/rex/plugins/zextract.lua and add
--   require("plugins.zextract")
-- to init.lua. Bindings (herdr parity, prefix ctrl+s):
--   ctrl+s>f        profile "open"   (current pane, all types)
--   ctrl+s>shift+f  profile "tab"    (whole tab)
--   ctrl+s>u        profile "url"    (current pane, URLs/IPs only)
--   ctrl+s>shift+u  profile "url-tab"
-- Profiles live in ~/.config/zextract/config.toml under [profiles.<n>]
-- (https://github.com/codingfragments/herdr-zextract, doc/config-reference.md).
--
-- Self-contained: uses lib.env (login PATH, rex CLI path) when present
-- and falls back to inline equivalents otherwise.

-- ── Environment helpers (lib/env.lua when present, else inline) ──
local login_path = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
local rex_cli = "rex"
do
  local ok, env = pcall(require, "lib.env")
  if ok and env and env.path and env.path ~= "" then login_path = env.path end
  if ok and env and env.rex_cli and env.rex_cli ~= "" then
    rex_cli = env.rex_cli
  else
    -- lib.env missing: try the app bundle location, the usual install
    -- of the Rex server on macOS.
    local bundle = (os.getenv("HOME") or "") ..
      "/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex"
    local f = io.open(bundle, "r")
    if f then f:close() rex_cli = bundle end
  end
end

-- The host-api version this launcher needs (the binary's `--host-api`;
-- bump when the launcher starts relying on something new).
local NEED_HOST_API = 1

-- POSIX sh, so it works on macOS and Linux hosts alike (the layer may
-- run on a remote host). Checks the binary, then execs it. Env comes
-- from `env` before sh, so the check and the run see the same context.
local PREFLIGHT = [[
if ! command -v herdr-zextract >/dev/null 2>&1; then
  printf '\n  zextract: herdr-zextract not found on this host.\n\n'
  printf '  Install it:  mise use -g github:codingfragments/herdr-zextract\n\n'
  printf '  Press Enter to close. '
  read -r _
  exit 0
fi
api=$(herdr-zextract --host-api 2>/dev/null || echo 0)
if [ "$api" -lt %d ]; then
  printf '\n  zextract: herdr-zextract is too old (host-api %%s, need %d).\n\n' "$api"
  printf '  Update it:  mise up herdr-zextract\n\n'
  printf '  Press Enter to close. '
  read -r _
  exit 0
fi
exec herdr-zextract
]]

local function popup_command(sid, block, window, profile)
  return {
    "env",
    "PATH=" .. login_path,
    "ZEXTRACT_HOST=rex",
    "REX_CLI=" .. rex_cli,
    "REX_SESSION=" .. sid,
    "ZEXTRACT_SOURCE_BLOCK=" .. block,
    "ZEXTRACT_SOURCE_WINDOW=" .. window,
    "ZEXTRACT_PROFILE=" .. profile,
    "/bin/sh", "-c", (PREFLIGHT):format(NEED_HOST_API, NEED_HOST_API),
    "sh",
  }
end

rex.action{
  name  = "zextract",
  title = "zextract",
  description = "Open zextract over the focused pane: fuzzy-find and act on URLs, paths, commands and more in its scrollback. Optional profile (grab scope/type filter) from ~/.config/zextract/config.toml.",
  args  = { profile = "string?" },
  run   = function(ctx, args)
    -- The source pane: what the user is looking at, read BEFORE the
    -- layer takes focus. Inside the popup, REX_BLOCK is the popup
    -- itself, so this must be passed explicitly.
    local v = rex.session.view{ session_id = ctx.session_id }
    local source_block, window
    for _, w in ipairs(v.windows or {}) do
      if w.window_id == v.active_window_id then
        source_block = w.focused_block_id
        window = w.window_id
      end
    end
    if not source_block or not window then
      error("no focused pane to grab from", 0)
    end

    local layer, err = rex.session.new_layer{
      session_id = ctx.session_id,
      bounds     = { 0.1, 0.1, 0.8, 0.8 },  -- herdr's 80% x 80% popup
      layout     = rex.layout.block{
        flavor  = "com.superlogical.terminal.shell",
        options = {
          command = popup_command(
            ctx.session_id, source_block, window, args.profile or "open"
          ),
          shell = "none",
        },
      },
      focus = true,
    }
    if err then error(err, 0) end
    return { layer_id = layer.layer_id, block_ids = layer.block_ids, source_block = source_block }
  end,
}

-- ── Bindings (herdr parity) ──────────────────────────────────────
rex.bind("ctrl+s>f", "zextract", { profile = "open" })
rex.bind("ctrl+s>shift+f", "zextract", { profile = "tab" })
rex.bind("ctrl+s>u", "zextract", { profile = "url" })
rex.bind("ctrl+s>shift+u", "zextract", { profile = "url-tab" })
-- Any further profile needs no packaging: bind it with its name, e.g.
--   rex.bind("ctrl+s>z", "zextract", { profile = "custom0" })
