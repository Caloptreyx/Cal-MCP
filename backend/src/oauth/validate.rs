use base64::Engine;
use sha2::Digest;

pub const MAX_REDIRECT_URIS: usize = 10;
pub const MAX_REDIRECT_URI_LEN: usize = 2048;
pub const MAX_CLIENT_NAME: usize = 100;
pub const MAX_STATE_LEN: usize = 2048;

/// Schemes that must never be redirected to, whatever the client registered: script and
/// document schemes, local files, and network schemes other than https and loopback http.
const FORBIDDEN_SCHEMES: [&str; 12] = [
    "javascript",
    "data",
    "file",
    "vbscript",
    "blob",
    "about",
    "filesystem",
    "view-source",
    "ftp",
    "ws",
    "wss",
    "mailto",
];

pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Checks a redirect URI at registration. Allowed: `https`, `http` on a loopback host, and
/// private-use app schemes such as `cursor://` or `vscode://` (RFC 8252 section 7.1), which
/// native clients need. Fragments are not allowed (RFC 6749 section 3.1.2).
pub fn redirect_uri(value: &str) -> Result<(), String> {
    if value.len() > MAX_REDIRECT_URI_LEN {
        return Err(format!(
            "redirect URIs must be at most {MAX_REDIRECT_URI_LEN} characters"
        ));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("redirect URIs must not contain whitespace".into());
    }

    let url = url::Url::parse(value).map_err(|_| format!("{value} is not an absolute URI"))?;
    if url.fragment().is_some() {
        return Err(format!("{value} must not contain a fragment"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!("{value} must not contain credentials"));
    }

    match url.scheme() {
        "https" => {
            if url.host_str().is_none_or(str::is_empty) {
                return Err(format!("{value} has no host"));
            }
            Ok(())
        }
        "http" => {
            if url.host_str().is_some_and(is_loopback_host) {
                Ok(())
            } else {
                Err(format!(
                    "{value} uses http; only https or http on localhost is allowed"
                ))
            }
        }
        scheme if FORBIDDEN_SCHEMES.contains(&scheme) => {
            Err(format!("the {scheme}: scheme is not allowed"))
        }
        _ => Ok(()),
    }
}

/// Host shown on the consent page so the user can see where the code is sent.
pub fn redirect_host(value: &str) -> String {
    match url::Url::parse(value) {
        Ok(url) => match url.host_str() {
            Some(host) if !host.is_empty() => match url.port() {
                Some(port) if url.scheme() == "http" => format!("{host}:{port}"),
                _ => host.to_string(),
            },
            _ => format!("{}:", url.scheme()),
        },
        Err(_) => value.to_string(),
    }
}

/// Whether the redirect URI only reaches the user's own machine.
pub fn is_loopback_redirect(value: &str) -> bool {
    url::Url::parse(value)
        .ok()
        .is_some_and(|url| url.scheme() == "http" && url.host_str().is_some_and(is_loopback_host))
}

pub fn client_name(value: Option<&str>) -> Result<String, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok("MCP client".into());
    };

    if value.chars().any(char::is_control) {
        return Err("client_name must not contain control characters".into());
    }

    Ok(value.chars().take(MAX_CLIENT_NAME).collect())
}

fn is_pkce_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
}

/// A code verifier or challenge: 43 to 128 unreserved characters (RFC 7636 section 4.1).
pub fn pkce_value(value: &str) -> bool {
    (43..=128).contains(&value.len()) && value.chars().all(is_pkce_char)
}

/// `BASE64URL(SHA256(verifier)) == challenge`, compared in constant time.
pub fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    if !pkce_value(verifier) {
        return false;
    }

    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));

    constant_time_eq(computed.as_bytes(), challenge.as_bytes())
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Compares a `resource` parameter (RFC 8707) with this server's resource identifier. Scheme and
/// host are case-insensitive and a trailing slash is tolerated; everything else must match.
pub fn resource_matches(requested: &str, resource: &str) -> bool {
    fn normalize(value: &str) -> Option<String> {
        let url = url::Url::parse(value.trim()).ok()?;
        if url.fragment().is_some() {
            return None;
        }

        let mut normalized = format!(
            "{}{}",
            url.origin().ascii_serialization(),
            url.path().trim_end_matches('/')
        );
        if let Some(query) = url.query() {
            normalized.push('?');
            normalized.push_str(query);
        }
        Some(normalized)
    }

    match (normalize(requested), normalize(resource)) {
        (Some(requested), Some(resource)) => requested == resource,
        _ => false,
    }
}
