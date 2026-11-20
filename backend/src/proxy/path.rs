const PATH_CHARS: &[u8] = b"/-._~%!$&'()*+,;=:@";

pub fn check(path: &str) -> Result<(), String> {
    if !(path.starts_with("/api/client/") || path.starts_with("/api/admin/")) {
        return Err(format!(
            "Path must start with /api/client/ or /api/admin/, got \"{path}\""
        ));
    }

    for segment in path[1..].split('/') {
        if segment.is_empty() {
            return Err("Path contains an empty segment".into());
        }
        // Percent-encoding must not hide a dot segment or another separator.
        let decoded = crate::access::percent_decode(segment);
        if decoded == "." || decoded == ".." {
            return Err("Path must not contain . or .. segments".into());
        }
        if decoded.contains(['/', '\\', '?', '#']) {
            return Err(format!(
                "Path segment \"{segment}\" must not encode a slash, backslash, ? or #"
            ));
        }
        if decoded.contains(['{', '}']) {
            return Err(format!(
                "Replace the placeholder \"{segment}\" in the path with a real value"
            ));
        }
    }

    let decoded: Vec<String> = path.split('/').map(crate::access::percent_decode).collect();
    if decoded.join("/").starts_with(&format!("/api/client/extensions/{}/", crate::PACKAGE)) {
        return Err("The MCP endpoint cannot call itself".into());
    }

    if let Some(bad) = path.chars().find(|c| {
        !(c.is_ascii_alphanumeric() || (c.is_ascii() && PATH_CHARS.contains(&(*c as u8))))
    }) {
        return Err(format!(
            "Path contains the character {bad:?}; send query parameters through `query`"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn accepts_ordinary_panel_paths() {
        assert!(check("/api/client/servers/0a1b2c3d/files/list").is_ok());
        assert!(check("/api/admin/users/5f0e").is_ok());
        assert!(check("/api/client/servers/0a1b2c3d/backups/My%20Backup").is_ok());
    }

    #[test]
    fn refuses_traversal_and_encoded_separators() {
        for path in [
            "/api/client/../admin/users",
            "/api/client/./servers",
            "/api/client/%2e%2e/admin/users",
            "/api/client/%2E%2E/admin/users",
            "/api/client/.%2e/admin/users",
            "/api/client/servers%2f..%2f..%2fadmin/users",
            "/api/client/servers%5Cadmin",
            "/api/client/servers%3Fx=1",
            "/api/client//servers",
            "/api/client/servers/%7Bserver%7D",
            "/api/Admin/users",
            "/api/%61dmin/users",
            "/api/client/servers?x=1",
        ] {
            assert!(check(path).is_err(), "{path}");
        }
    }

    #[test]
    fn cannot_reach_its_own_routes_even_encoded() {
        for path in [
            "/api/client/extensions/dev.caloptreyx.calmcp/mcp/inner",
            "/api/client/extensions/dev%2Ecaloptreyx.calmcp/oauth/grants",
            "/api/client/extensions/dev.caloptreyx.calmc%70/restrictions/x",
        ] {
            assert!(check(path).is_err(), "{path}");
        }
    }
}
