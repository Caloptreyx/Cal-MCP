const PATH_CHARS: &[u8] = b"/-._~%!$&'()*+,;=:@";

pub fn check(path: &str) -> Result<(), String> {
    if !(path.starts_with("/api/client/") || path.starts_with("/api/admin/")) {
        return Err(format!(
            "Path must start with /api/client/ or /api/admin/, got \"{path}\""
        ));
    }

    if path.starts_with(&format!("/api/client/extensions/{}/", crate::PACKAGE)) {
        return Err("The MCP endpoint cannot call itself".into());
    }

    for segment in path[1..].split('/') {
        if segment.is_empty() {
            return Err("Path contains an empty segment".into());
        }
        if segment == "." || segment == ".." {
            return Err("Path must not contain . or .. segments".into());
        }
        if segment.contains(['{', '}']) {
            return Err(format!(
                "Replace the placeholder \"{segment}\" in the path with a real value"
            ));
        }
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
