use super::protocol::Era;
use crate::{access::Access, settings::ExtensionSettingsData};
use serde_json::{Map, Value, json};

pub const MODERN: &str = "2026-07-28";
pub const LEGACY: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

fn instructions(settings: &ExtensionSettingsData, access: Access) -> String {
    let mut text = format!(
        "{} manages a {} game panel with the permissions of the API key it was connected with. \
Use the dedicated tools for everyday server work: servers and power (list_servers, get_server, power_server), the console (run_command, send_command, read_console), files (list_files, read_file, edit_file, write_file), backups, schedules and startup variables. \
Change existing files with edit_file; write_file replaces the whole file, so never pass it a part that read_file returned. \
Everything else the panel API offers is reachable in three steps: search_api to find the endpoint, describe_api to see its parameters and body, then read_api for GET or write_api for POST, PUT, PATCH and DELETE. \
Paths look like /api/client/servers/{{server}}/backups; replace every {{placeholder}} with a real value before calling. \
Admin endpoints under /api/admin only work when the key has admin permissions. \
Server consoles and files are also readable as resources: calagopus://servers/{{server}}/console and calagopus://servers/{{server}}/files/{{path}}.",
        settings.display_title, settings.panel_name
    );
    if let Some(limits) = access.describe() {
        text.push(' ');
        text.push_str(limits);
    }
    text
}

pub fn supported_versions() -> Vec<&'static str> {
    std::iter::once(MODERN).chain(LEGACY).collect()
}

pub fn server_info(settings: &ExtensionSettingsData) -> Value {
    json!({
        "name": settings.integration_name,
        "title": settings.display_title,
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn capabilities() -> Value {
    json!({
        "tools": { "listChanged": false },
        "prompts": { "listChanged": false },
        "resources": { "subscribe": false, "listChanged": false },
        "completions": {},
    })
}

pub fn initialize(
    params: &Map<String, Value>,
    settings: &ExtensionSettingsData,
    access: Access,
) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let version = LEGACY
        .iter()
        .find(|version| **version == requested)
        .copied()
        .unwrap_or(LEGACY[0]);

    json!({
        "protocolVersion": version,
        "capabilities": capabilities(),
        "serverInfo": server_info(settings),
        "instructions": instructions(settings, access),
    })
}

pub fn discover(settings: &ExtensionSettingsData, access: Access) -> Value {
    json!({
        "supportedVersions": supported_versions(),
        "capabilities": capabilities(),
        "instructions": instructions(settings, access),
    })
}

pub fn finish(era: &Era, result: Value, settings: &ExtensionSettingsData) -> Value {
    match (era, result) {
        (Era::Modern, Value::Object(mut result)) => {
            result.insert("resultType".into(), json!("complete"));
            result.insert(
                "_meta".into(),
                json!({ "io.modelcontextprotocol/serverInfo": server_info(settings) }),
            );
            Value::Object(result)
        }
        (_, result) => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_eras_declare_prompts_resources_and_completions() {
        let settings = ExtensionSettingsData::default();
        for capabilities in [
            initialize(&Map::new(), &settings, Access::NONE)["capabilities"].clone(),
            discover(&settings, Access::NONE)["capabilities"].clone(),
        ] {
            assert_eq!(capabilities["prompts"], json!({ "listChanged": false }));
            assert_eq!(
                capabilities["resources"],
                json!({ "subscribe": false, "listChanged": false })
            );
            assert_eq!(capabilities["completions"], json!({}));
        }
    }
}
