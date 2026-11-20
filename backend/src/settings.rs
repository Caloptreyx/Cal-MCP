use crate::access::Access;
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use shared::{
    State,
    extensions::settings::{
        ExtensionSettings, SettingsDeserializeExt, SettingsDeserializer, SettingsSerializeExt,
        SettingsSerializer,
    },
};
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;

pub const DEFAULT_INTEGRATION_NAME: &str = "calagopus";
pub const DEFAULT_DISPLAY_TITLE: &str = "Cal MCP";
pub const DEFAULT_PANEL_NAME: &str = "Calagopus";

pub const MAX_INTEGRATION_NAME: usize = 64;
pub const MAX_DISPLAY_TITLE: usize = 96;
pub const MAX_PANEL_NAME: usize = 64;

/// How many API keys one user may keep restrictions for.
pub const MAX_KEY_RESTRICTIONS_PER_USER: usize = 100;

/// The access a user chose for one of their own API keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRestriction {
    pub user: Uuid,
    #[serde(flatten)]
    pub access: Access,
}

#[derive(Clone, Debug)]
pub struct ExtensionSettingsData {
    /// The name clients use to register the MCP server, for example in a
    /// `claude mcp add` command or the `mcpServers` object of a JSON config.
    pub integration_name: CompactString,
    /// The title MCP clients display for this server.
    pub display_title: CompactString,
    /// The panel name mentioned in the instructions and setup prompt.
    pub panel_name: CompactString,
    /// Restrictions every MCP connection gets, whatever key it uses.
    pub restrictions: Access,
    /// Restrictions users chose for their own API keys, by API key UUID.
    pub key_restrictions: BTreeMap<Uuid, KeyRestriction>,
}

impl Default for ExtensionSettingsData {
    fn default() -> Self {
        Self {
            integration_name: DEFAULT_INTEGRATION_NAME.into(),
            display_title: DEFAULT_DISPLAY_TITLE.into(),
            panel_name: DEFAULT_PANEL_NAME.into(),
            restrictions: Access::NONE,
            key_restrictions: BTreeMap::new(),
        }
    }
}

impl ExtensionSettingsData {
    /// The access of a connection: the panel-wide restrictions plus whatever
    /// the user chose for the API key it connects with.
    pub fn access_for(&self, api_key: Option<Uuid>) -> Access {
        let chosen = api_key
            .and_then(|key| self.key_restrictions.get(&key))
            .map(|entry| entry.access)
            .unwrap_or_default();

        self.restrictions.union(chosen)
    }

    /// The settings every signed-in user may see: branding and the panel-wide
    /// restrictions, without anyone's per-key choices.
    pub fn public(&self) -> PublicSettings {
        PublicSettings {
            integration_name: self.integration_name.clone(),
            display_title: self.display_title.clone(),
            panel_name: self.panel_name.clone(),
            read_only: self.restrictions.read_only,
            client_only: self.restrictions.client_only,
        }
    }
}

#[derive(Clone, Debug, ToSchema, Serialize)]
pub struct PublicSettings {
    pub integration_name: CompactString,
    pub display_title: CompactString,
    pub panel_name: CompactString,
    /// Every MCP connection is read-only.
    pub read_only: bool,
    /// Every MCP connection is blocked from /api/admin.
    pub client_only: bool,
}

/// Trims and validates a client-facing integration name. Names have to be safe
/// to place in a shell command and as an object key in client configs.
pub fn normalize_integration_name(value: &str) -> Option<CompactString> {
    let value = value.trim();

    if value.is_empty() || value.chars().count() > MAX_INTEGRATION_NAME {
        return None;
    }

    let mut chars = value.chars();
    let first = chars.next()?;

    if !first.is_ascii_alphanumeric() {
        return None;
    }

    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return None;
    }

    Some(value.into())
}

/// Trims and validates free-form branding text. Control characters are rejected
/// because these values are interpolated into MCP instructions.
pub fn normalize_display_text(value: &str, max: usize) -> Option<CompactString> {
    let value = value.trim();

    if value.is_empty() || value.chars().count() > max || value.chars().any(char::is_control) {
        return None;
    }

    Some(value.into())
}

fn flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn parse_key_restrictions(value: &str) -> BTreeMap<Uuid, KeyRestriction> {
    match serde_json::from_str(value) {
        Ok(map) => map,
        Err(err) => {
            tracing::warn!("ignoring unreadable Cal MCP key restrictions: {err}");
            BTreeMap::new()
        }
    }
}

#[async_trait::async_trait]
impl SettingsSerializeExt for ExtensionSettingsData {
    async fn serialize(
        &self,
        serializer: SettingsSerializer,
    ) -> Result<SettingsSerializer, anyhow::Error> {
        Ok(serializer
            .write_raw_setting("integration_name", self.integration_name.clone())
            .write_raw_setting("display_title", self.display_title.clone())
            .write_raw_setting("panel_name", self.panel_name.clone())
            .write_raw_setting("read_only", flag(self.restrictions.read_only))
            .write_raw_setting("client_only", flag(self.restrictions.client_only))
            .write_raw_setting(
                "key_restrictions",
                serde_json::to_string(&self.key_restrictions)?,
            ))
    }
}

pub struct ExtensionSettingsDataDeserializer;

#[async_trait::async_trait]
impl SettingsDeserializeExt for ExtensionSettingsDataDeserializer {
    async fn deserialize_boxed(
        &self,
        mut deserializer: SettingsDeserializer<'_>,
    ) -> Result<ExtensionSettings, anyhow::Error> {
        let default = ExtensionSettingsData::default();
        let mut take_flag =
            |key: &str| deserializer.take_raw_setting(key).as_deref() == Some("true");
        let restrictions = Access {
            read_only: take_flag("read_only"),
            client_only: take_flag("client_only"),
        };

        Ok(Box::new(ExtensionSettingsData {
            integration_name: deserializer
                .take_raw_setting("integration_name")
                .and_then(|value| normalize_integration_name(&value))
                .unwrap_or(default.integration_name),
            display_title: deserializer
                .take_raw_setting("display_title")
                .and_then(|value| normalize_display_text(&value, MAX_DISPLAY_TITLE))
                .unwrap_or(default.display_title),
            panel_name: deserializer
                .take_raw_setting("panel_name")
                .and_then(|value| normalize_display_text(&value, MAX_PANEL_NAME))
                .unwrap_or(default.panel_name),
            restrictions,
            key_restrictions: deserializer
                .take_raw_setting("key_restrictions")
                .map(|value| parse_key_restrictions(&value))
                .unwrap_or_default(),
        }))
    }
}

/// Reads the settings and returns an error when they cannot be loaded. The MCP
/// endpoint uses this so a broken settings store never drops restrictions.
pub async fn load_result(state: &State) -> Result<ExtensionSettingsData, anyhow::Error> {
    let settings = state.settings.get().await?;

    settings
        .get_extension_settings::<ExtensionSettingsData>(crate::PACKAGE)
        .cloned()
}

/// Reads the current branding settings, falling back to the defaults when the
/// settings store cannot be read or does not contain this extension yet.
pub async fn load(state: &State) -> ExtensionSettingsData {
    match load_result(state).await {
        Ok(settings) => settings,
        Err(err) => {
            tracing::warn!("failed to load Cal MCP settings, using defaults: {err:#}");
            ExtensionSettingsData::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn key_restrictions_add_to_the_panel_wide_ones() {
        let mut settings = ExtensionSettingsData {
            restrictions: Access {
                read_only: false,
                client_only: true,
            },
            ..Default::default()
        };
        settings.key_restrictions.insert(
            key(1),
            KeyRestriction {
                user: key(9),
                access: Access {
                    read_only: true,
                    client_only: false,
                },
            },
        );

        assert_eq!(
            settings.access_for(Some(key(1))),
            Access {
                read_only: true,
                client_only: true
            }
        );
        assert_eq!(
            settings.access_for(Some(key(2))),
            settings.restrictions
        );
        assert_eq!(settings.access_for(None), settings.restrictions);
    }

    #[test]
    fn key_restrictions_round_trip_through_json() {
        let mut map = BTreeMap::new();
        map.insert(
            key(1),
            KeyRestriction {
                user: key(2),
                access: Access {
                    read_only: true,
                    client_only: false,
                },
            },
        );
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(parse_key_restrictions(&json), map);
        assert!(json.contains("\"read_only\":true"), "{json}");
    }

    #[test]
    fn unreadable_key_restrictions_are_ignored() {
        assert!(parse_key_restrictions("not json").is_empty());
    }

    #[test]
    fn public_settings_hide_key_choices() {
        let mut settings = ExtensionSettingsData::default();
        settings.key_restrictions.insert(
            key(1),
            KeyRestriction {
                user: key(2),
                access: Access {
                    read_only: true,
                    client_only: true,
                },
            },
        );
        let public = serde_json::to_value(settings.public()).unwrap();
        assert_eq!(
            public,
            serde_json::json!({
                "integration_name": "calagopus",
                "display_title": "Cal MCP",
                "panel_name": "Calagopus",
                "read_only": false,
                "client_only": false,
            })
        );
    }
}
