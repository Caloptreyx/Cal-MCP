//! `completion/complete` for prompt arguments and resource template variables.

use crate::{
    panel::{Failure, Panel, get_json, is_server_id, object},
    prompts,
    resources::{self, uri},
};
use serde_json::{Map, Value, json};

/// The spec caps a completion at 100 values.
const MAX_VALUES: usize = 100;
const MAX_SEARCH: usize = 128;

/// Where the suggestions for an argument come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Short IDs of the servers the caller can access.
    ServerIds,
    /// Names of the servers the caller can access.
    ServerNames,
    /// Paths on the server named by the `server` context argument.
    Paths,
    Fixed(&'static [&'static str]),
    Nothing,
}

struct Completion {
    values: Vec<String>,
    total: Option<i64>,
    has_more: bool,
}

impl Completion {
    fn empty() -> Self {
        Self {
            values: Vec::new(),
            total: Some(0),
            has_more: false,
        }
    }
}

fn resource_source(template: &str, argument: &str) -> Result<Source, Failure> {
    match (template, argument) {
        (uri::FILES_TEMPLATE | uri::CONSOLE_TEMPLATE, "server") => Ok(Source::ServerIds),
        (uri::FILES_TEMPLATE, "path") => Ok(Source::Paths),
        (uri::FILES_TEMPLATE | uri::CONSOLE_TEMPLATE, _) => Ok(Source::Nothing),
        _ => Err(Failure::Invalid(format!(
            "Unknown resource template \"{template}\""
        ))),
    }
}

pub async fn complete(panel: &impl Panel, params: &Map<String, Value>) -> Result<Value, Failure> {
    let reference = params
        .get("ref")
        .and_then(Value::as_object)
        .ok_or_else(|| Failure::Invalid("completion/complete needs params.ref".into()))?;
    let argument = params
        .get("argument")
        .and_then(Value::as_object)
        .ok_or_else(|| Failure::Invalid("completion/complete needs params.argument".into()))?;
    let name = argument
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Failure::Invalid("params.argument.name must be a string".into()))?;
    let value = match argument.get("value") {
        None | Some(Value::Null) => "",
        Some(Value::String(value)) => value.as_str(),
        Some(_) => {
            return Err(Failure::Invalid(
                "params.argument.value must be a string".into(),
            ));
        }
    };
    let empty = Map::new();
    let context = params
        .get("context")
        .and_then(|context| context.get("arguments"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    let text = |key: &str| {
        reference
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let source = match text("type") {
        "ref/prompt" => prompts::completion_source(text("name"), name)?,
        "ref/resource" => resource_source(text("uri"), name)?,
        _ => {
            return Err(Failure::Invalid(
                "params.ref.type must be ref/prompt or ref/resource".into(),
            ));
        }
    };

    let completion = match source {
        Source::ServerIds => servers(panel, value, true).await,
        Source::ServerNames => servers(panel, value, false).await,
        Source::Paths => paths(panel, value, context).await,
        Source::Fixed(options) => fixed(options, value),
        Source::Nothing => Completion::empty(),
    };

    let mut result = json!({ "values": completion.values, "hasMore": completion.has_more });
    if let Some(total) = completion.total {
        result["total"] = json!(total);
    }
    Ok(json!({ "completion": result }))
}

fn starts_with(candidate: &str, prefix: &str) -> bool {
    candidate.to_lowercase().starts_with(&prefix.to_lowercase())
}

fn fixed(options: &[&str], value: &str) -> Completion {
    let values: Vec<String> = options
        .iter()
        .filter(|option| starts_with(option, value))
        .map(|option| option.to_string())
        .collect();
    Completion {
        total: Some(values.len() as i64),
        values,
        has_more: false,
    }
}

/// Uses the panel's own server search, so only servers the caller can see
/// are ever suggested.
async fn servers(panel: &impl Panel, value: &str, ids: bool) -> Completion {
    let mut query = object(json!({ "page": 1, "per_page": MAX_VALUES }));
    let search: String = value.trim().chars().take(MAX_SEARCH).collect();
    if !search.is_empty() {
        query.insert("search".into(), json!(search));
    }
    let Ok(body) = get_json(panel, "/api/client/servers", Some(&query), None).await else {
        return Completion::empty();
    };

    let servers = body
        .pointer("/servers/data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut values: Vec<String> = Vec::new();
    for server in &servers {
        let key = if ids { "uuid_short" } else { "name" };
        if let Some(value) = server.get(key).and_then(Value::as_str)
            && !values.iter().any(|known| known == value)
        {
            values.push(value.to_string());
        }
    }
    // Prefix matches first; the panel's search also matches inside names.
    values.sort_by_key(|candidate| !starts_with(candidate, &search));

    let total = body
        .pointer("/servers/total")
        .and_then(Value::as_i64)
        .unwrap_or(servers.len() as i64);
    Completion {
        total: ids.then_some(total),
        has_more: total > servers.len() as i64,
        values,
    }
}

async fn paths(panel: &impl Panel, value: &str, context: &Map<String, Value>) -> Completion {
    let Some(server) = context
        .get("server")
        .and_then(Value::as_str)
        .filter(|server| is_server_id(server))
        .map(str::to_ascii_lowercase)
    else {
        return Completion::empty();
    };
    let value = value.trim_start_matches('/');
    let (directory, prefix) = value.rsplit_once('/').unwrap_or(("", value));
    let Ok(directory) = uri::clean_path(directory) else {
        return Completion::empty();
    };
    let Ok(body) = resources::list_directory(panel, &server, &directory, 1, None).await else {
        return Completion::empty();
    };

    let mut values = Vec::new();
    for entry in body
        .pointer("/entries/data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = entry.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !starts_with(name, prefix) {
            continue;
        }
        let mut path = if directory.is_empty() {
            name.to_string()
        } else {
            format!("{directory}/{name}")
        };
        if entry.get("directory").and_then(Value::as_bool) == Some(true) {
            path.push('/');
        }
        values.push(path);
    }
    values.truncate(MAX_VALUES);

    // Only the first page of a large directory was searched.
    let listed_everything = body
        .pointer("/entries/total")
        .and_then(Value::as_i64)
        .is_none_or(|total| total <= resources::ENTRIES_PER_PAGE);
    Completion {
        total: listed_everything.then_some(values.len() as i64),
        has_more: !listed_everything,
        values,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::mock::MockPanel;

    async fn run(panel: &MockPanel, params: Value) -> Result<Value, Failure> {
        complete(panel, &object(params)).await
    }

    fn servers_reply(total: i64, servers: &[(&str, &str)]) -> Value {
        let data: Vec<Value> = servers
            .iter()
            .map(|(id, name)| json!({ "uuid_short": id, "name": name }))
            .collect();
        json!({ "servers": { "total": total, "per_page": 100, "page": 1, "data": data } })
    }

    #[tokio::test]
    async fn completes_server_ids_for_resource_templates() {
        let panel = MockPanel::default().json(
            "/api/client/servers?page=1&per_page=100&search=0a",
            servers_reply(150, &[("1f0a0000", "Lobby"), ("0a1b2c3d", "Survival")]),
        );
        let result = run(
            &panel,
            json!({
                "ref": { "type": "ref/resource", "uri": uri::CONSOLE_TEMPLATE },
                "argument": { "name": "server", "value": "0a" },
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            result,
            json!({ "completion": { "values": ["0a1b2c3d", "1f0a0000"], "hasMore": true, "total": 150 } })
        );
    }

    #[tokio::test]
    async fn completes_server_names_for_prompts() {
        let panel = MockPanel::default().json(
            "/api/client/servers?page=1&per_page=100",
            servers_reply(
                3,
                &[
                    ("0a1b2c3d", "Survival"),
                    ("11111111", "Creative"),
                    ("22222222", "Survival"),
                ],
            ),
        );
        let result = run(
            &panel,
            json!({
                "ref": { "type": "ref/prompt", "name": "diagnose_crash" },
                "argument": { "name": "server", "value": "" },
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            result["completion"]["values"],
            json!(["Survival", "Creative"])
        );
        assert_eq!(result["completion"]["hasMore"], json!(false));
    }

    #[tokio::test]
    async fn completes_paths_inside_the_typed_directory() {
        let panel = MockPanel::default().json(
            "/api/client/servers/0a1b2c3d/files/list?directory=/plugins&page=1&per_page=100",
            json!({ "entries": { "total": 3, "per_page": 100, "page": 1, "data": [
                { "name": "Essentials", "directory": true },
                { "name": "essentials.jar", "directory": false },
                { "name": "WorldEdit.jar", "directory": false },
            ] } }),
        );
        let result = run(
            &panel,
            json!({
                "ref": { "type": "ref/resource", "uri": uri::FILES_TEMPLATE },
                "argument": { "name": "path", "value": "/plugins/ess" },
                "context": { "arguments": { "server": "0A1B2C3D" } },
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            result["completion"]["values"],
            json!(["plugins/Essentials/", "plugins/essentials.jar"])
        );

        // Without a server there is nothing to look in.
        let result = run(
            &MockPanel::default(),
            json!({
                "ref": { "type": "ref/resource", "uri": uri::FILES_TEMPLATE },
                "argument": { "name": "path", "value": "" },
            }),
        )
        .await
        .unwrap();
        assert_eq!(result["completion"]["values"], json!([]));
    }

    #[tokio::test]
    async fn completes_fixed_choices_without_the_panel() {
        let panel = MockPanel::default();
        let result = run(
            &panel,
            json!({
                "ref": { "type": "ref/prompt", "name": "setup_minecraft_server" },
                "argument": { "name": "software", "value": "PA" },
            }),
        )
        .await
        .unwrap();
        assert_eq!(result["completion"]["values"], json!(["paper"]));
        assert!(panel.requested().is_empty());
    }

    #[tokio::test]
    async fn rejects_unknown_references() {
        for params in [
            json!({ "ref": { "type": "ref/prompt", "name": "nope" }, "argument": { "name": "server", "value": "" } }),
            json!({ "ref": { "type": "ref/resource", "uri": "file:///{path}" }, "argument": { "name": "path", "value": "" } }),
            json!({ "ref": { "type": "ref/tool", "name": "x" }, "argument": { "name": "a", "value": "" } }),
            json!({ "ref": { "type": "ref/prompt", "name": "diagnose_crash" } }),
        ] {
            assert!(
                matches!(
                    run(&MockPanel::default(), params.clone()).await,
                    Err(Failure::Invalid(_))
                ),
                "{params}"
            );
        }
    }

    #[tokio::test]
    async fn panel_errors_complete_to_nothing() {
        let panel =
            MockPanel::default().error("/api/client/servers?page=1&per_page=100", 403, "denied");
        let result = run(
            &panel,
            json!({
                "ref": { "type": "ref/prompt", "name": "diagnose_crash" },
                "argument": { "name": "server", "value": "" },
            }),
        )
        .await
        .unwrap();
        assert_eq!(result["completion"]["values"], json!([]));
    }
}
