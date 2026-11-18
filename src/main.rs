mod actions;
mod config;
mod grab;
mod host;
mod matcher;
mod picker;

use std::rc::Rc;

/// Env prefix for this tool's host settings (`ZEXTRACT_HOST`, …), see
/// [`host::detect`].
const ENV_PREFIX: &str = "ZEXTRACT";

fn run() -> Result<(), String> {
    let host: Rc<dyn host::Host> = Rc::from(host::detect(ENV_PREFIX)?);

    let config_missing = config::is_missing();
    let config = config::Config::load(host.name());
    // [patterns].disable before any grab-profile-specific or
    // launch-allowlist adjustment - the fixed baseline every grabber in
    // the Ctrl-G cycle merges its own `disable` list into (see
    // `GrabCycler::new`).
    let raw_disabled = config.disabled.clone();

    config.log(
        config::LogLevel::Debug,
        &format!(
            "host {}: source pane={:?} tab={:?}",
            host.name(),
            host.source().pane_id,
            host.source().tab_id
        ),
    );

    // Per-keybind override: which named profile (grab scope, pattern
    // allowlist, query pre-fill) to use. The launcher action in
    // herdr-plugin.toml only ever sets ZEXTRACT_PROFILE to a *name* -
    // the profile's actual values live in the user's own config.toml
    // under [profiles.<name>], never in plugin packaging.
    let profile_name = std::env::var("ZEXTRACT_PROFILE").unwrap_or_else(|_| "open".to_string());
    let profile = config.resolve_profile(&profile_name);
    let initial_grab_name = profile.grab.clone().unwrap_or_else(|| "quick".to_string());
    let allowed: Option<std::collections::HashSet<String>> = profile
        .patterns
        .as_ref()
        .map(|p| p.iter().cloned().collect());

    config.log(
        config::LogLevel::Debug,
        &format!(
            "profile {profile_name:?}: grab={initial_grab_name:?} grab_cycle={:?} patterns={:?}",
            profile.grab_cycle, profile.patterns
        ),
    );

    // Launch-time default for the picker's preview split - `[ui].preview`
    // unless `[profiles.<name>].preview` overrides it for this keybind.
    let preview_open = config.resolve_preview_open(&profile);
    config.log(
        config::LogLevel::Debug,
        &format!(
            "preview: open={preview_open} open_width={:?} closed_width={:?}",
            config.ui.preview_open_width, config.ui.preview_closed_width
        ),
    );

    let grab_cycler = grab::GrabCycler::new(
        &config,
        &raw_disabled,
        allowed.as_ref(),
        &initial_grab_name,
        profile.grab_cycle.as_deref(),
        Rc::clone(&host),
    );
    let grab::CaptureResult {
        matches,
        pane_texts,
    } = grab_cycler
        .capture_current()
        .map_err(|e| format!("grab failed: {e}"))?;
    // An empty result is NOT a reason to skip the picker: the popup
    // stays open so the user can Ctrl-G into another grab profile (or
    // check the query pre-fill) instead of watching it flash shut.
    // State::new seeds a footer hint when `matches` is empty.

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

    let selection = picker::run(picker::LaunchArgs {
        matches,
        pane_texts,
        custom_tags: &custom_tags,
        config_missing,
        config: &config,
        initial_query: &initial_query,
        preview_open,
        grab_cycler,
    })
    .map_err(|e| format!("picker failed: {e}"))?;
    match selection {
        picker::PickerResult::Selected(matches, verb) => {
            let refs: Vec<&matcher::Match> = matches.iter().collect();
            // Insert always targets the host's source pane, regardless
            // of which pane a multi-pane match came from.
            match actions::execute_batch(verb, &refs, host.as_ref(), &config) {
                actions::Outcome::Done(msg) => println!("{msg}"),
                actions::Outcome::Failed(msg) => println!("error: {msg}"),
            }
        }
        picker::PickerResult::Cancelled => println!("(cancelled)"),
    }
    Ok(())
}

fn main() {
    // Plain flags for launchers and install scripts, answered before any
    // host detection or terminal setup.
    match std::env::args().nth(1).as_deref() {
        Some("--version") | Some("-V") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--host-api") => {
            println!("{}", host::HOST_API);
            return;
        }
        _ => {}
    }
    if let Err(message) = run() {
        eprintln!("herdr-zextract error: {message}");
    }
}
