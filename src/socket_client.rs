use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use serde_json::Value;

/// Send one newline-delimited JSON request over `$HERDR_SOCKET_PATH`
/// and block for the matching response line, on a **fresh connection
/// every call**.
///
/// Herdr's socket server closes the connection after serving exactly
/// one request - confirmed directly: a second request sent over an
/// already-used connection gets `BrokenPipeError` even milliseconds
/// later, no idle time needed. This surfaced as two separate-looking
/// bugs before being traced to the same root cause: Phase 4's insert
/// action needing its own fresh connection (blamed on the picker
/// session's idle time), and Phase 7's `tab-scan` grab failing every
/// `pane.read` after its `pane.list` call (no idle time at all - a
/// handful of milliseconds). This function is now the *only* way
/// anything in the plugin talks to the socket, so the bug class can't
/// recur by construction - there is no persistent-connection API left
/// to accidentally reuse.
pub fn request(socket_path: &str, method: &str, params: Value) -> std::io::Result<Value> {
    let stream = UnixStream::connect(socket_path)?;
    let mut reader = BufReader::new(stream);

    let req = serde_json::json!({
        "id": format!("herdr-zextract-{method}"),
        "method": method,
        "params": params,
    });
    let mut line = serde_json::to_string(&req).map_err(std::io::Error::other)?;
    line.push('\n');
    reader.get_mut().write_all(line.as_bytes())?;

    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    let response: Value =
        serde_json::from_str(response_line.trim_end()).map_err(std::io::Error::other)?;

    if let Some(error) = response.get("error") {
        return Err(std::io::Error::other(format!(
            "herdr socket error: {error}"
        )));
    }

    response
        .get("result")
        .cloned()
        .ok_or_else(|| std::io::Error::other("herdr socket response missing \"result\""))
}
