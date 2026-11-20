//! `calagopus://servers/{server}/...` resource URIs.

use crate::panel::is_server_id;

pub const PREFIX: &str = "calagopus://servers/";
pub const FILES_TEMPLATE: &str = "calagopus://servers/{server}/files/{+path}";
pub const CONSOLE_TEMPLATE: &str = "calagopus://servers/{server}/console{?lines}";

pub const DEFAULT_LINES: u32 = 200;
pub const MAX_LINES: u32 = 1000;

#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// The last `lines` lines of the server console.
    Console { server: String, lines: u32 },
    /// A path that is read as a file, or listed when it turns out to be a directory.
    /// `path` has no leading slash, for example `plugins/config.yml`.
    File { server: String, path: String },
    /// A directory listing; `path` is empty for the root.
    Directory {
        server: String,
        path: String,
        page: u32,
    },
}

// RFC 3986 pchar without `%` and `+` (some decoders read `+` as a space).
fn keep(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~!$&'()*,;=:@".contains(&byte)
}

fn encode_segment(segment: &str, out: &mut String) {
    for byte in segment.bytes() {
        if keep(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
}

/// `plugins/My Plugin` -> `plugins/My%20Plugin`
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for (index, segment) in path.split('/').enumerate() {
        if index > 0 {
            out.push('/');
        }
        encode_segment(segment, &mut out);
    }
    out
}

fn decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let byte = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .ok_or_else(|| format!("Invalid percent-encoding in \"{text}\""))?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("\"{text}\" does not decode to UTF-8"))
}

/// Splits a decoded path into clean segments, refusing anything that could
/// step outside the server's directory.
pub fn clean_path(path: &str) -> Result<String, String> {
    if path.chars().any(char::is_control) {
        return Err("File paths must not contain control characters".into());
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" => {}
            "." | ".." => return Err("File paths must not contain . or .. segments".into()),
            segment => segments.push(segment),
        }
    }
    Ok(segments.join("/"))
}

pub fn console(server: &str) -> String {
    format!("{PREFIX}{server}/console")
}

pub fn file(server: &str, path: &str) -> String {
    format!("{PREFIX}{server}/files/{}", encode_path(path))
}

pub fn directory(server: &str, path: &str, page: u32) -> String {
    let mut uri = format!("{PREFIX}{server}/files/");
    if !path.is_empty() {
        uri.push_str(&encode_path(path));
        uri.push('/');
    }
    if page > 1 {
        uri.push_str(&format!("?page={page}"));
    }
    uri
}

fn query_number(query: Option<&str>, name: &str, default: u32, max: u32) -> Result<u32, String> {
    let Some(query) = query else {
        return Ok(default);
    };
    let mut value = default;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        match pair.split_once('=') {
            Some((key, number)) if key == name => {
                value = number
                    .parse::<u32>()
                    .ok()
                    .filter(|number| (1..=max).contains(number))
                    .ok_or_else(|| format!("`{name}` must be a number from 1 to {max}"))?;
            }
            _ => {
                return Err(format!(
                    "Unknown query parameter \"{pair}\"; only `{name}` is supported"
                ));
            }
        }
    }
    Ok(value)
}

pub fn parse(uri: &str) -> Result<Target, String> {
    let rest = uri
        .strip_prefix(PREFIX)
        .ok_or_else(|| format!("Resource URIs start with {PREFIX}, got \"{uri}\""))?;
    if rest.contains('#') {
        return Err("Resource URIs must not have a fragment".into());
    }
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (rest, None),
    };
    let (server, tail) = rest.split_once('/').unwrap_or((rest, ""));
    if !is_server_id(server) {
        return Err(format!(
            "\"{server}\" is not a server UUID or short ID (see list_servers)"
        ));
    }
    let server = server.to_ascii_lowercase();

    if tail == "console" {
        return Ok(Target::Console {
            server,
            lines: query_number(query, "lines", DEFAULT_LINES, MAX_LINES)?,
        });
    }

    let raw = match tail {
        "files" => "",
        tail => tail.strip_prefix("files/").ok_or_else(|| {
            format!("Unknown resource \"{uri}\"; servers have /console and /files/<path>")
        })?,
    };
    let path = clean_path(&decode(raw)?)?;

    if raw.is_empty() || raw.ends_with('/') {
        return Ok(Target::Directory {
            server,
            path,
            page: query_number(query, "page", 1, u32::MAX)?,
        });
    }
    if query.is_some() {
        return Err("File URIs take no query parameters".into());
    }
    Ok(Target::File { server, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0a1b2c3d";

    #[test]
    fn parses_console_uris() {
        assert_eq!(
            parse(&console(ID)),
            Ok(Target::Console {
                server: ID.into(),
                lines: DEFAULT_LINES
            })
        );
        assert_eq!(
            parse("calagopus://servers/0A1B2C3D/console?lines=50"),
            Ok(Target::Console {
                server: ID.into(),
                lines: 50
            })
        );
        assert!(parse("calagopus://servers/0a1b2c3d/console?lines=0").is_err());
        assert!(parse("calagopus://servers/0a1b2c3d/console?lines=1001").is_err());
        assert!(parse("calagopus://servers/0a1b2c3d/console?tail=5").is_err());
    }

    #[test]
    fn parses_files_and_directories() {
        assert_eq!(
            parse("calagopus://servers/0a1b2c3d/files/plugins/My%20Plugin/config.yml"),
            Ok(Target::File {
                server: ID.into(),
                path: "plugins/My Plugin/config.yml".into()
            })
        );
        // `{+path}` may also be expanded with an encoded slash.
        assert_eq!(
            parse("calagopus://servers/0a1b2c3d/files/plugins%2Fconfig.yml"),
            Ok(Target::File {
                server: ID.into(),
                path: "plugins/config.yml".into()
            })
        );
        for root in [
            "calagopus://servers/0a1b2c3d/files",
            "calagopus://servers/0a1b2c3d/files/",
        ] {
            assert_eq!(
                parse(root),
                Ok(Target::Directory {
                    server: ID.into(),
                    path: String::new(),
                    page: 1
                })
            );
        }
        assert_eq!(
            parse("calagopus://servers/0a1b2c3d/files/world//region/?page=3"),
            Ok(Target::Directory {
                server: ID.into(),
                path: "world/region".into(),
                page: 3
            })
        );
    }

    #[test]
    fn refuses_traversal_and_foreign_uris() {
        for uri in [
            "calagopus://servers/0a1b2c3d/files/../etc/passwd",
            "calagopus://servers/0a1b2c3d/files/a/%2E%2E/b",
            "calagopus://servers/0a1b2c3d/files/a%00b",
            "calagopus://servers/0a1b2c3d/files/a%zz",
            "calagopus://servers/0a1b2c3d/files/a.txt?page=2",
            "calagopus://servers/0a1b2c3d/backups",
            "calagopus://servers/not-an-id/console",
            "file:///etc/passwd",
            "calagopus://servers/0a1b2c3d/console#x",
        ] {
            assert!(parse(uri).is_err(), "{uri} should be refused");
        }
    }

    #[test]
    fn built_uris_round_trip() {
        let path = "plugins/Essentials X/100% sure+?.yml";
        assert_eq!(
            parse(&file(ID, path)),
            Ok(Target::File {
                server: ID.into(),
                path: path.into()
            })
        );
        assert_eq!(
            directory(ID, "world/data", 2),
            "calagopus://servers/0a1b2c3d/files/world/data/?page=2"
        );
        assert_eq!(directory(ID, "", 1), "calagopus://servers/0a1b2c3d/files/");
    }
}
