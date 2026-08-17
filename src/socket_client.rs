use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use serde_json::Value;

pub struct SocketClient {
    stream: BufReader<UnixStream>,
    next_id: u64,
}

impl SocketClient {
    pub fn connect(socket_path: &str) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket_path)?;
        Ok(Self {
            stream: BufReader::new(stream),
            next_id: 1,
        })
    }

    /// Sends one newline-delimited JSON request and blocks for the matching response line.
    pub fn request(&mut self, method: &str, params: Value) -> std::io::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = serde_json::json!({
            "id": format!("herdr-zextract-{id}"),
            "method": method,
            "params": params,
        });

        let mut line = serde_json::to_string(&request).map_err(std::io::Error::other)?;
        line.push('\n');
        self.stream.get_mut().write_all(line.as_bytes())?;

        let mut response_line = String::new();
        self.stream.read_line(&mut response_line)?;

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
}
