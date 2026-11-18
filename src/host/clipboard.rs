//! Copy text to the user's clipboard, whichever host the tool runs on.
//!
//! First the system clipboard (`arboard`). Where there is none, as in a
//! headless or remote Linux session behind Rex or ssh, fall back to an
//! OSC 52 escape sequence written to the controlling terminal, which
//! the terminal (Rex, herdr's client, …) forwards to the local
//! clipboard if it supports OSC 52.

use std::io::Write;

/// Copy `text`. `Err` only when both the system clipboard and the OSC 52
/// fallback failed.
pub fn copy(text: &str) -> Result<(), String> {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        Ok(()) => Ok(()),
        Err(clip_err) => osc52(text)
            .map_err(|osc_err| format!("{clip_err}; OSC 52 fallback failed too: {osc_err}")),
    }
}

/// Write an OSC 52 "set clipboard" sequence to `/dev/tty`.
fn osc52(text: &str) -> Result<(), String> {
    let mut tty = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .map_err(|e| format!("no terminal: {e}"))?;
    tty.write_all(osc52_sequence(text).as_bytes())
        .and_then(|_| tty.flush())
        .map_err(|e| e.to_string())
}

fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// Standard base64 with padding (RFC 4648); small enough not to need a
/// dependency.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "input {input:?}");
        }
    }

    #[test]
    fn osc52_sequence_shape() {
        assert_eq!(osc52_sequence("hi"), "\x1b]52;c;aGk=\x07");
    }
}
