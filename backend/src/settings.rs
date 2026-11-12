use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use shared::{
    State,
    extensions::settings::{
        ExtensionSettings, SettingsDeserializeExt, SettingsDeserializer, SettingsSerializeExt,
        SettingsSerializer,
    },
};
use utoipa::ToSchema;

pub const DEFAULT_INTEGRATION_NAME: &str = "calagopus";
pub const DEFAULT_DISPLAY_TITLE: &str = "Cal MCP";
pub const DEFAULT_PANEL_NAME: &str = "Calagopus";

#[derive(Clone, Debug, ToSchema, Serialize, Deserialize)]
pub struct ExtensionSettingsData {
    /// The name clients use to register the MCP server, for example in a
    /// `claude mcp add` command or the `mcpServers` object of a JSON config.
    pub integration_name: CompactString,
    /// The title MCP clients display for this server.
    pub display_title: CompactString,
    /// The panel name mentioned in the instructions and setup prompt.
    pub panel_name: CompactString,
}

impl Default for ExtensionSettingsData {
    fn default() -> Self {
        Self {
            integration_name: DEFAULT_INTEGRATION_NAME.into(),
            display_title: DEFAULT_DISPLAY_TITLE.into(),
            panel_name: DEFAULT_PANEL_NAME.into(),
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
            .write_raw_setting("panel_name", self.panel_name.clone()))
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

        Ok(Box::new(ExtensionSettingsData {
            integration_name: non_empty(
                deserializer.take_raw_setting("integration_name"),
                default.integration_name,
            ),
            display_title: non_empty(
                deserializer.take_raw_setting("display_title"),
                default.display_title,
            ),
            panel_name: non_empty(
                deserializer.take_raw_setting("panel_name"),
                default.panel_name,
            ),
        }))
    }
}

fn non_empty(value: Option<CompactString>, fallback: CompactString) -> CompactString {
    match value {
        Some(value) if !value.trim().is_empty() => value,
        _ => fallback,
    }
}

/// Reads the current branding settings, falling back to the defaults when the
/// settings store cannot be read or does not contain this extension yet.
pub async fn load(state: &State) -> ExtensionSettingsData {
    match state.settings.get().await {
        Ok(settings) => settings
            .find_extension_settings::<ExtensionSettingsData>()
            .map(Clone::clone)
            .unwrap_or_default(),
        Err(_) => ExtensionSettingsData::default(),
    }
}
