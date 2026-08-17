mod actions;
mod config;
mod matcher;
mod picker;
mod socket_client;

use socket_client::SocketClient;

/// Reads the previously-focused pane id from either launch path: a real
/// plugin-pane invocation sets `HERDR_PLUGIN_CONTEXT_JSON.focused_pane_id`;
/// a `[[keys.command]]` custom-command popup (used for manual testing ahead
/// of Phase 6's real plugin keybind) sets `HERDR_ACTIVE_PANE_ID` directly.
fn focused_pane_id() -> Result<String, String> {
    if let Ok(context_json) = std::env::var("HERDR_PLUGIN_CONTEXT_JSON") {
        let context: serde_json::Value = serde_json::from_str(&context_json)
            .map_err(|e| format!("invalid context JSON: {e}"))?;
        return context
            .get("focused_pane_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                "context JSON has no focused_pane_id (nothing was focused before this popup opened)"
                    .to_string()
            });
    }
    std::env::var("HERDR_ACTIVE_PANE_ID").map_err(|_| {
        "neither HERDR_PLUGIN_CONTEXT_JSON nor HERDR_ACTIVE_PANE_ID is set".to_string()
    })
}

fn run() -> Result<(), String> {
    let pane_id = focused_pane_id()?;

    let socket_path = std::env::var("HERDR_SOCKET_PATH")
        .map_err(|_| "HERDR_SOCKET_PATH is not set".to_string())?;
    let mut client = SocketClient::connect(&socket_path)
        .map_err(|e| format!("failed to connect to {socket_path}: {e}"))?;

    let result = client
        .request(
            "pane.read",
            serde_json::json!({
                "pane_id": pane_id,
                "source": "recent_unwrapped",
            }),
        )
        .map_err(|e| format!("pane.read failed: {e}"))?;

    let text = result
        .get("read")
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .ok_or("pane.read response had no \"read.text\" field")?;

    let config_missing = config::is_missing();
    let config = config::Config::load();
    let matches = matcher::extract_with_config(text, &config);
    if matches.is_empty() {
        println!("--- no matches in scrollback of {pane_id} ---");
        return Ok(());
    }

    let custom_tags: Vec<String> = config.custom.iter().map(|cp| cp.name.clone()).collect();
    let selection = picker::run(matches, &custom_tags, config_missing)
        .map_err(|e| format!("picker failed: {e}"))?;
    match selection {
        picker::PickerResult::Selected(matches, verb) => {
            let refs: Vec<&matcher::Match> = matches.iter().collect();
            match actions::execute_batch(verb, &refs, &pane_id) {
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
