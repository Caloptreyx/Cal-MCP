//! First-class tools for backups, schedules and startup variables.

use super::{
    Ctx,
    args::{boolean, identifier, integer, optional_text, server, text, to_map},
    output::request,
};
use axum::http::Method;
use serde_json::{Map, Value, json};

fn base(server: &str) -> String {
    format!("/api/client/servers/{server}")
}

fn page_query(args: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut query = to_map(json!({
        "page": integer(args, "page", 1, 1, i64::MAX)?,
        "per_page": integer(args, "per_page", 25, 1, 100)?,
    }));
    if let Some(search) = optional_text(args, "search")?.filter(|search| !search.is_empty()) {
        query.insert("search".into(), json!(search));
    }
    Ok(query)
}

fn string_list(args: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("`{key}` must be a list of strings"))
            })
            .collect(),
        Some(_) => Err(format!("`{key}` must be a list of strings")),
    }
}

pub async fn list_backups(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let query = page_query(args)?;
    request(
        ctx,
        Method::GET,
        &format!("{}/backups", base(server)),
        Some(&query),
        None,
    )
    .await
}

pub async fn create_backup(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let mut body = to_map(json!({ "ignored_files": string_list(args, "ignored_files")? }));
    if let Some(name) = optional_text(args, "name")?.filter(|name| !name.is_empty()) {
        body.insert("name".into(), json!(name));
    }

    let reply = request(
        ctx,
        Method::POST,
        &format!("{}/backups", base(server)),
        None,
        Some(&Value::Object(body)),
    )
    .await?;
    Ok(format!(
        "Started a backup of server {server}. It runs in the background: list_backups shows it as completed once it has finished.\n{reply}"
    ))
}

pub async fn restore_backup(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let backup = identifier(args, "backup")?;
    request(
        ctx,
        Method::POST,
        &format!("{}/backups/{backup}/restore", base(server)),
        None,
        Some(&json!({
            "truncate_directory": boolean(args, "truncate_directory")?,
            "restore_startup": boolean(args, "restore_startup")?,
        })),
    )
    .await?;
    Ok(format!(
        "Started restoring backup {backup} on server {server}. The server cannot be used until the restore has finished; get_server shows its state."
    ))
}

pub async fn delete_backup(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let backup = identifier(args, "backup")?;
    request(
        ctx,
        Method::DELETE,
        &format!("{}/backups/{backup}", base(server)),
        None,
        None,
    )
    .await?;
    Ok(format!("Deleted backup {backup} of server {server}."))
}

pub async fn list_schedules(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let query = page_query(args)?;
    request(
        ctx,
        Method::GET,
        &format!("{}/schedules", base(server)),
        Some(&query),
        None,
    )
    .await
}

pub async fn create_schedule(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let name = text(args, "name")?;
    let enabled = match args.get("enabled") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(enabled)) => *enabled,
        Some(_) => return Err("`enabled` must be true or false".into()),
    };

    let mut triggers = match args.get("triggers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(triggers)) if triggers.iter().all(Value::is_object) => triggers.clone(),
        Some(_) => return Err("`triggers` must be a list of trigger objects".into()),
    };
    if let Some(cron) = optional_text(args, "cron")?.filter(|cron| !cron.trim().is_empty()) {
        triggers.push(json!({ "type": "cron", "schedule": cron.trim() }));
    }
    let condition = match args.get("condition") {
        None | Some(Value::Null) => json!({ "type": "none" }),
        Some(condition @ Value::Object(_)) => condition.clone(),
        Some(_) => return Err("`condition` must be an object".into()),
    };
    let steps = match args.get("steps") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(steps)) if steps.iter().all(|step| step.get("type").is_some()) => {
            steps.clone()
        }
        Some(_) => {
            return Err(
                "`steps` must be a list of action objects, each with a \"type\" such as \"send_command\""
                    .into(),
            );
        }
    };
    if steps.len() > 100 {
        return Err("`steps` can hold at most 100 actions".into());
    }

    let created = request(
        ctx,
        Method::POST,
        &format!("{}/schedules", base(server)),
        None,
        Some(&json!({
            "name": name,
            "enabled": enabled,
            "triggers": triggers,
            "condition": condition,
        })),
    )
    .await?;
    let schedule = serde_json::from_str::<Value>(&created)
        .ok()
        .and_then(|value| value.pointer("/schedule/uuid")?.as_str().map(str::to_string))
        .ok_or_else(|| format!("The panel created the schedule but did not return its ID: {created}"))?;

    for (index, step) in steps.iter().enumerate() {
        if let Err(err) = request(
            ctx,
            Method::POST,
            &format!("{}/schedules/{schedule}/steps", base(server)),
            None,
            Some(&json!({ "action": step, "order": index + 1 })),
        )
        .await
        {
            return Err(format!(
                "Created schedule \"{name}\" ({schedule}) with {index} of {} steps; step {} failed: {err}. \
Fix the step and add it with write_api (POST /api/client/servers/{server}/schedules/{schedule}/steps), or delete the schedule.",
                steps.len(),
                index + 1
            ));
        }
    }

    Ok(format!(
        "Created schedule \"{name}\" ({schedule}) on server {server} with {} trigger{} and {} step{}{}.",
        triggers.len(),
        if triggers.len() == 1 { "" } else { "s" },
        steps.len(),
        if steps.len() == 1 { "" } else { "s" },
        if enabled { "" } else { ", disabled" }
    ))
}

pub async fn run_schedule(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let schedule = identifier(args, "schedule")?;
    request(
        ctx,
        Method::POST,
        &format!("{}/schedules/{schedule}/trigger", base(server)),
        None,
        Some(&json!({ "skip_condition": boolean(args, "skip_condition")? })),
    )
    .await?;
    Ok(format!(
        "Started schedule {schedule} on server {server}. It runs in the background; read_console or list_schedules show what it did."
    ))
}

pub async fn list_variables(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    request(
        ctx,
        Method::GET,
        &format!("{}/startup/variables", base(server)),
        None,
        None,
    )
    .await
}

pub async fn update_variables(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let Some(Value::Object(values)) = args.get("variables") else {
        return Err(
            "`variables` must be an object of environment variable names and new values, for example {\"SERVER_JARFILE\": \"server.jar\"}"
                .into(),
        );
    };
    if values.is_empty() {
        return Err("`variables` must name at least one variable".into());
    }

    let mut variables = Vec::with_capacity(values.len());
    for (name, value) in values {
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Number(number) => number.to_string(),
            Value::Bool(flag) => flag.to_string(),
            _ => return Err(format!("The value of {name} must be a string, number or boolean")),
        };
        variables.push(json!({ "env_variable": name, "value": value }));
    }

    request(
        ctx,
        Method::PUT,
        &format!("{}/startup/variables", base(server)),
        None,
        Some(&json!({ "variables": variables })),
    )
    .await?;
    let names: Vec<&str> = values.keys().map(String::as_str).collect();
    Ok(format!(
        "Updated {} on server {server}. Restart the server for the new values to take effect.",
        names.join(", ")
    ))
}
