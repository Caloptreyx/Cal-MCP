use axum::http::Method;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// What one MCP connection may do. Restrictions only ever add up: the
/// panel-wide settings, the connecting API key's own choice and an OAuth
/// grant's scope are combined with [`Access::union`], so no source can loosen
/// another one's limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Access {
    /// Only GET requests and read-only tools.
    #[serde(default)]
    pub read_only: bool,
    /// No requests to /api/admin.
    #[serde(default)]
    pub client_only: bool,
}

impl Access {
    pub const NONE: Self = Self {
        read_only: false,
        client_only: false,
    };

    pub fn union(self, other: Self) -> Self {
        Self {
            read_only: self.read_only || other.read_only,
            client_only: self.client_only || other.client_only,
        }
    }

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }

    /// Checks one panel API request. Every tool reaches the panel through the
    /// proxy, which calls this before sending anything.
    pub fn check_request(self, method: &Method, path: &str) -> Result<(), String> {
        if self.client_only && (path == "/api/admin" || path.starts_with("/api/admin/")) {
            return Err(
                "This MCP connection is client-only, so /api/admin endpoints are blocked.".into(),
            );
        }

        if self.read_only && method != Method::GET {
            return Err(format!(
                "This MCP connection is read-only, so {method} requests are blocked. Only GET requests and read tools are allowed."
            ));
        }

        if self.read_only && issues_credentials(path) {
            return Err(
                "This MCP connection is read-only, so endpoints that hand out console, upload or node credentials (or change two-factor setup) are blocked.".into(),
            );
        }

        Ok(())
    }

    /// Whether a tool with the given `readOnlyHint` may be listed and called.
    pub fn allows_tool(self, read_only_tool: bool) -> bool {
        !self.read_only || read_only_tool
    }

    /// A sentence for the server instructions, when anything is restricted.
    pub fn describe(self) -> Option<&'static str> {
        match (self.read_only, self.client_only) {
            (false, false) => None,
            (true, false) => Some(
                "This connection is read-only: only GET requests and read tools are available.",
            ),
            (false, true) => {
                Some("This connection is client-only: /api/admin endpoints are blocked.")
            }
            (true, true) => Some(
                "This connection is read-only and client-only: only GET requests to /api/client and read tools are available.",
            ),
        }
    }
}

/// GET endpoints that hand out credentials which can change things outside the panel API (a
/// Wings websocket token with console and power rights, a signed upload URL, node and database
/// agent tokens), or that write state despite being GET (two-factor setup stores a new secret).
/// A read-only connection must not reach them. `*` stands for one path segment.
const CREDENTIAL_ENDPOINTS: &[&[&str]] = &[
    &["api", "client", "servers", "*", "websocket"],
    &["api", "client", "servers", "*", "files", "upload"],
    &["api", "client", "account", "two-factor"],
    &["api", "admin", "nodes", "*", "token"],
    &["api", "admin", "nodes", "*", "config"],
    &["api", "admin", "database-agent-hosts", "*", "token"],
    &["api", "admin", "database-agent-hosts", "*", "config"],
];

pub(crate) fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let decoded = (bytes[index] == b'%')
            .then(|| segment.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match decoded {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Compares decoded, case-folded segments so encoding or case tricks cannot slip past.
fn issues_credentials(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| percent_decode(segment).to_ascii_lowercase())
        .collect();

    CREDENTIAL_ENDPOINTS.iter().any(|pattern| {
        pattern.len() == segments.len()
            && pattern
                .iter()
                .zip(&segments)
                .all(|(expected, actual)| *expected == "*" || expected == actual)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const READ_ONLY: Access = Access {
        read_only: true,
        client_only: false,
    };
    const CLIENT_ONLY: Access = Access {
        read_only: false,
        client_only: true,
    };

    #[test]
    fn union_never_loosens() {
        assert_eq!(Access::NONE.union(Access::NONE), Access::NONE);
        assert_eq!(
            READ_ONLY.union(CLIENT_ONLY),
            Access {
                read_only: true,
                client_only: true
            }
        );
        assert_eq!(READ_ONLY.union(Access::NONE), READ_ONLY);
        assert_eq!(Access::NONE.union(CLIENT_ONLY), CLIENT_ONLY);
    }

    #[test]
    fn no_restrictions_allow_everything() {
        for method in [Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(
                Access::NONE
                    .check_request(&method, "/api/admin/users")
                    .is_ok()
            );
        }
    }

    #[test]
    fn read_only_blocks_every_write_method() {
        assert!(READ_ONLY.check_request(&Method::GET, "/api/client/servers").is_ok());
        assert!(READ_ONLY.check_request(&Method::GET, "/api/admin/users").is_ok());
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            let err = READ_ONLY
                .check_request(&method, "/api/client/servers/abc/power")
                .unwrap_err();
            assert!(err.contains("read-only"), "{err}");
        }
    }

    #[test]
    fn client_only_blocks_admin_paths_for_every_method() {
        for method in [Method::GET, Method::POST, Method::DELETE] {
            assert!(CLIENT_ONLY.check_request(&method, "/api/admin/users").is_err());
            assert!(CLIENT_ONLY.check_request(&method, "/api/admin").is_err());
            assert!(CLIENT_ONLY.check_request(&method, "/api/client/servers").is_ok());
        }
        // A client path that merely contains "admin" is still a client path.
        assert!(
            CLIENT_ONLY
                .check_request(&Method::GET, "/api/client/servers/admin")
                .is_ok()
        );
    }

    #[test]
    fn read_only_blocks_gets_that_hand_out_write_credentials() {
        for path in [
            "/api/client/servers/0a1b2c3d/websocket",
            "/api/client/servers/0a1b2c3d/websocket/",
            "/api/client/servers/0a1b2c3d/files/upload",
            "/api/client/servers/0a1b2c3d/files/%75pload",
            "/api/client/servers/0a1b2c3d/WebSocket",
            "/api/client/account/two-factor",
            "/api/admin/nodes/5f0e/token",
            "/api/admin/nodes/5f0e/config",
            "/api/admin/database-agent-hosts/5f0e/token",
            "/api/admin/database-agent-hosts/5f0e/config",
        ] {
            let err = READ_ONLY.check_request(&Method::GET, path).unwrap_err();
            assert!(err.contains("read-only"), "{path}: {err}");
            // Without read-only these are ordinary requests the panel's permissions decide.
            assert!(Access::NONE.check_request(&Method::GET, path).is_ok(), "{path}");
        }

        for path in [
            "/api/client/servers/0a1b2c3d",
            "/api/client/servers/0a1b2c3d/files/list",
            "/api/client/servers/0a1b2c3d/files/download",
            "/api/client/servers/websocket",
            "/api/client/account",
            "/api/admin/nodes/5f0e",
        ] {
            assert!(READ_ONLY.check_request(&Method::GET, path).is_ok(), "{path}");
        }
    }

    #[test]
    fn read_only_hides_write_tools_only() {
        assert!(READ_ONLY.allows_tool(true));
        assert!(!READ_ONLY.allows_tool(false));
        assert!(CLIENT_ONLY.allows_tool(false));
        assert!(Access::NONE.allows_tool(false));
    }
}
