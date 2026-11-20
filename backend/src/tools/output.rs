use super::Ctx;
use crate::proxy::{self, Reply};
use axum::http::Method;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// The most text one tool result carries.
pub const MAX_TEXT: usize = 100_000;
const BINARY_TYPES: [&str; 12] = [
    "image/", "audio/", "video/", "font/", "zip", "gzip", "tar", "pdf", "wasm", "7z", "rar",
    "msgpack",
];

/// Cuts `text` to at most `max` bytes without splitting a character.
pub fn floor_boundary(text: &str, max: usize) -> usize {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

pub fn truncate(text: String) -> String {
    if text.len() <= MAX_TEXT {
        return text;
    }

    let end = floor_boundary(&text, MAX_TEXT);
    format!(
        "{}\n\n[Truncated: showing the first {end} of {} bytes. Narrow the request with query parameters or pagination.]",
        &text[..end],
        text.len()
    )
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data).as_slice())
}

fn as_text<'a>(content_type: Option<&str>, body: &'a [u8]) -> Option<&'a str> {
    let binary_type = content_type.is_some_and(|content_type| {
        let content_type = content_type.to_ascii_lowercase();
        BINARY_TYPES.iter().any(|kind| content_type.contains(kind))
    });
    if binary_type || body.contains(&0) {
        return None;
    }
    std::str::from_utf8(body).ok()
}

pub fn render(reply: &Reply) -> Result<String, String> {
    let body = if reply.body.is_empty() {
        format!("HTTP {} with an empty body", reply.status)
    } else {
        match as_text(reply.content_type.as_deref(), &reply.body) {
            Some(text) => text.to_string(),
            None => format!(
                "Binary response ({}, {} bytes)",
                reply.content_type.as_deref().unwrap_or("unknown type"),
                reply.body.len()
            ),
        }
    };

    if reply.status >= 400 {
        Err(truncate(format!("HTTP {}: {body}", reply.status)))
    } else {
        Ok(truncate(body))
    }
}

pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

pub async fn request(
    ctx: &Ctx,
    method: Method,
    path: &str,
    query: Option<&Map<String, Value>>,
    body: Option<&Value>,
) -> Result<String, String> {
    render(&proxy::call(ctx, method, path, query, body).await?)
}

/// Like [`request`], and also returns the response parsed as JSON (whole,
/// even when the text had to be truncated), or `None` when it is not JSON.
pub async fn request_json(
    ctx: &Ctx,
    method: Method,
    path: &str,
    query: Option<&Map<String, Value>>,
    body: Option<&Value>,
) -> Result<(String, Option<Value>), String> {
    let reply = proxy::call(ctx, method, path, query, body).await?;
    let text = render(&reply)?;
    Ok((text, serde_json::from_slice(&reply.body).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_boundary_never_splits_a_character() {
        let text = "aé"; // é is two bytes
        assert_eq!(floor_boundary(text, 2), 1);
        assert_eq!(floor_boundary(text, 3), 3);
        assert_eq!(floor_boundary(text, 10), 3);
    }

    #[test]
    fn sha256_matches_a_known_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
