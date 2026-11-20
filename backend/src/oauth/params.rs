use base64::Engine;
use std::collections::HashMap;

/// Form or query parameters. OAuth forbids sending a parameter more than once, so repeats are
/// remembered instead of silently picking one value.
#[derive(Debug, Default)]
pub struct Params {
    values: HashMap<String, String>,
    repeated: Vec<String>,
}

impl Params {
    pub fn parse(input: &str) -> Params {
        let mut params = Params::default();

        for (name, value) in url::form_urlencoded::parse(input.as_bytes()) {
            if params.values.contains_key(name.as_ref()) {
                if !params.repeated.iter().any(|repeated| repeated == name.as_ref()) {
                    params.repeated.push(name.into_owned());
                }
                continue;
            }
            params.values.insert(name.into_owned(), value.into_owned());
        }

        params
    }

    /// The value of a parameter; empty values count as missing (RFC 6749 section 3.1).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    pub fn is_repeated(&self, name: &str) -> bool {
        self.repeated.iter().any(|repeated| repeated == name)
    }

    pub fn first_repeated(&self) -> Option<&str> {
        self.repeated.first().map(String::as_str)
    }
}

/// Client credentials from an `Authorization: Basic` header (RFC 6749 section 2.3.1): both parts
/// are form-urlencoded before being joined and base64 encoded.
pub fn basic_credentials(header: &str) -> Option<(String, String)> {
    let (scheme, encoded) = header.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (id, secret) = decoded.split_once(':')?;

    let decode = |value: &str| {
        url::form_urlencoded::parse(format!("v={value}").as_bytes())
            .next()
            .map(|(_, value)| value.into_owned())
    };

    Some((decode(id)?, decode(secret)?))
}

/// The token of an `Authorization: Bearer` header.
pub fn bearer(header: &str) -> Option<&str> {
    let (scheme, token) = header.trim().split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|token| !token.is_empty())
}
