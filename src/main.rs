mod socket_client;

use socket_client::SocketClient;

fn run() -> Result<(), String> {
    let context_json = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .map_err(|_| "HERDR_PLUGIN_CONTEXT_JSON is not set".to_string())?;
    let context: serde_json::Value =
        serde_json::from_str(&context_json).map_err(|e| format!("invalid context JSON: {e}"))?;
    let pane_id = context
        .get("focused_pane_id")
        .and_then(|v| v.as_str())
        .ok_or(
            "context JSON has no focused_pane_id (nothing was focused before this popup opened)",
        )?;

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

    println!("--- scrollback of {pane_id} ---");
    print!("{text}");
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
