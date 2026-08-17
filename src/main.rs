mod actions;
mod config;
mod grab;
mod matcher;
mod picker;
mod socket_client;

/// Launch context: which pane/tab this popup was opened relative to.
struct LaunchContext {
    focused_pane_id: String,
    /// Empty when launched via the `HERDR_ACTIVE_PANE_ID` dev-testing
    /// fallback (no real tab context) - `tab-scan` degrades to finding
    /// zero panes in that case, which is fine for a manual dev keybind.
    tab_id: String,
}

/// Reads the launch context from either path: a real plugin-pane
/// invocation sets `HERDR_PLUGIN_CONTEXT_JSON`; a `[[keys.command]]`
/// custom-command popup (used for manual dev-testing ahead of a real
/// plugin keybind) sets `HERDR_ACTIVE_PANE_ID` directly.
fn launch_context() -> Result<LaunchContext, String> {
    if let Ok(context_json) = std::env::var("HERDR_PLUGIN_CONTEXT_JSON") {
        let context: serde_json::Value = serde_json::from_str(&context_json)
            .map_err(|e| format!("invalid context JSON: {e}"))?;
        let focused_pane_id = context
            .get("focused_pane_id")
            .and_then(|v| v.as_str())
            .ok_or(
                "context JSON has no focused_pane_id (nothing was focused before this popup opened)",
            )?
            .to_string();
        let tab_id = context
            .get("tab_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(LaunchContext {
            focused_pane_id,
            tab_id,
        });
    }
    let focused_pane_id = std::env::var("HERDR_ACTIVE_PANE_ID").map_err(|_| {
        "neither HERDR_PLUGIN_CONTEXT_JSON nor HERDR_ACTIVE_PANE_ID is set".to_string()
    })?;
    Ok(LaunchContext {
        focused_pane_id,
        tab_id: String::new(),
    })
}

fn run() -> Result<(), String> {
    let ctx = launch_context()?;

    let socket_path = std::env::var("HERDR_SOCKET_PATH")
        .map_err(|_| "HERDR_SOCKET_PATH is not set".to_string())?;

    let config_missing = config::is_missing();
    let mut config = config::Config::load();

    // Per-keybind override: which named profile (grab scope, pattern
    // allowlist, query pre-fill) to use. The launcher action in
    // herdr-plugin.toml only ever sets ZEXTRACT_PROFILE to a *name* -
    // the profile's actual values live in the user's own config.toml
    // under [profiles.<name>], never in plugin packaging.
    let profile_name = std::env::var("ZEXTRACT_PROFILE").unwrap_or_else(|_| "open".to_string());
    let profile = config.resolve_profile(&profile_name);

    let grab_profile = config.resolve_grab_profile(profile.grab.as_deref().unwrap_or("quick"));
    config.log(
        config::LogLevel::Debug,
        &format!(
            "profile {profile_name:?}: grab source={:?} lines={:?} patterns={:?}",
            grab_profile.source, grab_profile.lines, profile.patterns
        ),
    );
    config.disabled.extend(grab_profile.disable.iter().cloned());

    // Preview pane rendering itself is Phase 9 - resolved here anyway
    // so `[ui].preview`/`[profiles.<name>].preview` are both exercised
    // (and observable via log_level="debug") well before Phase 9 lands.
    let preview_open = config.resolve_preview_open(&profile);
    config.log(
        config::LogLevel::Debug,
        &format!(
            "preview: open={preview_open} open_width={:?} closed_width={:?}",
            config.ui.preview_open_width, config.ui.preview_closed_width
        ),
    );
    let captures = grab::capture(
        &grab_profile,
        &socket_path,
        &ctx.focused_pane_id,
        &ctx.tab_id,
    )
    .map_err(|e| format!("grab failed: {e}"))?;

    if let Some(patterns) = &profile.patterns {
        let allowed: std::collections::HashSet<String> = patterns.iter().cloned().collect();
        config.restrict_to(&allowed);
    }

    let multi_pane = captures.len() > 1;
    let mut matches = Vec::new();
    for cap in &captures {
        let mut found = matcher::extract_with_config(&cap.text, &config);
        // Pane-title prefix only makes sense (and is only computed)
        // when more than one pane actually contributed matches -
        // matches the original's "prefix omitted in single-pane mode".
        if multi_pane {
            for m in &mut found {
                m.fields
                    .insert("__pane_id".to_string(), cap.pane_id.clone());
                m.fields
                    .insert("__pane_title".to_string(), cap.title.clone());
            }
        }
        matches.extend(found);
    }
    if matches.is_empty() {
        println!("--- no matches found ---");
        return Ok(());
    }

    let custom_tags: Vec<String> = config.custom.iter().map(|cp| cp.name.clone()).collect();
    let initial_query = profile
        .type_filter
        .as_ref()
        .map(|tags| {
            tags.iter()
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let selection = picker::run(
        matches,
        &custom_tags,
        config_missing,
        &config,
        &initial_query,
    )
    .map_err(|e| format!("picker failed: {e}"))?;
    match selection {
        picker::PickerResult::Selected(matches, verb) => {
            let refs: Vec<&matcher::Match> = matches.iter().collect();
            // Insert always targets the pane the plugin was launched
            // from, regardless of which pane a multi-pane match came
            // from - ctx.focused_pane_id, not any per-match pane id.
            match actions::execute_batch(verb, &refs, &ctx.focused_pane_id, &config) {
                actions::Outcome::Done(msg) => println!("{msg}"),
                actions::Outcome::Failed(msg) => println!("error: {msg}"),
            }
        }
        picker::PickerResult::Cancelled => println!("(cancelled)"),
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("herdr-zextract error: {message}");
    }
    println!("\n-- press Enter to close --");
    let mut discard = String::new();
    let _ = std::io::stdin().read_line(&mut discard);
}
