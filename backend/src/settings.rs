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

pub const MAX_INTEGRATION_NAME: usize = 64;
pub const MAX_DISPLAY_TITLE: usize = 96;
pub const MAX_PANEL_NAME: usize = 64;

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
        }))
    }
}

/// Reads the settings and returns an error when they cannot be loaded. Routes
/// that are allowed to fall back to defaults should use [`load`].
pub async fn load_result(state: &State) -> Result<ExtensionSettingsData, anyhow::Error> {
    let settings = state.settings.get().await?;

    settings
        .get_extension_settings::<ExtensionSettingsData>(crate::PACKAGE)
        .map(Clone::clone)
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
