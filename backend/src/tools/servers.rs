use super::{
    Ctx, Output,
    args::{boolean, integer, optional_text, server, text, to_map},
    output::{request, request_json, strip_ansi, tail, truncate},
};
use crate::proxy;
use axum::http::Method;
use serde_json::{Map, Value, json};
use std::time::Duration;
use tokio::time::{Instant, sleep};

const POWER_ACTIONS: [&str; 4] = ["start", "stop", "restart", "kill"];
/// The most console lines the panel returns at once.
const MAX_CONSOLE_LINES: i64 = 1000;
const CONSOLE_POLL: Duration = Duration::from_millis(500);
const STATE_POLL: Duration = Duration::from_secs(1);
/// Without `wait_for`, run_command returns once the console was quiet this long.
const QUIET: Duration = Duration::from_millis(1500);

async fn state(ctx: &Ctx, server: &str) -> Option<String> {
    let (_, resources) = request_json(
        ctx,
        Method::GET,
        &format!("/api/client/servers/{server}/resources"),
        None,
        None,
    )
    .await
    .ok()?;

    resources?
        .pointer("/resources/state")?
        .as_str()
        .map(str::to_string)
}

pub async fn list(ctx: &Ctx, args: &Map<String, Value>) -> Result<Output, String> {
    let mut query = to_map(json!({
        "page": integer(args, "page", 1, 1, i64::MAX)?,
        "per_page": integer(args, "per_page", 25, 1, 100)?,
        "other": boolean(args, "other")?,
    }));
    if let Some(search) = optional_text(args, "search")?.filter(|search| !search.is_empty()) {
        query.insert("search".into(), json!(search));
    }

    let (text, json) =
        request_json(ctx, Method::GET, "/api/client/servers", Some(&query), None).await?;
    Ok(Output::structured(text, json))
}

pub async fn get(ctx: &Ctx, args: &Map<String, Value>) -> Result<Output, String> {
    let base = format!("/api/client/servers/{}", server(args)?);
    let (details_text, details) = request_json(ctx, Method::GET, &base, None, None).await?;
    let resources = request_json(ctx, Method::GET, &format!("{base}/resources"), None, None).await;

    let mut combined = match details {
        Some(Value::Object(details)) => details,
        _ => to_map(json!({ "server": details_text })),
    };
    combined.insert(
        "resources".into(),
        match resources {
            Ok((_, Some(Value::Object(mut resources)))) => match resources.remove("resources") {
                Some(inner) => inner,
                None => Value::Object(resources),
            },
            Ok((text, _)) => Value::String(text),
            Err(err) => json!({ "error": err }),
        },
    );

    let combined = Value::Object(combined);
    Ok(Output::structured(
        truncate(combined.to_string()),
        Some(combined),
    ))
}

pub async fn power(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let action = text(args, "action")?;
    if !POWER_ACTIONS.contains(&action) {
        return Err("`action` must be start, stop, restart or kill".into());
    }
    let wait = boolean(args, "wait")?;
    let timeout = integer(args, "timeout_seconds", 60, 1, 120)? as u64;

    request(
        ctx,
        Method::POST,
        &format!("/api/client/servers/{server}/power"),
        None,
        Some(&json!({ "action": action })),
    )
    .await?;
    if !wait {
        return Ok(format!("Sent {action} to server {server}."));
    }

    let target = match action {
        "start" | "restart" => "running",
        _ => "offline",
    };
    let started = Instant::now();
    // A restart only counts once the server has left the running state.
    let mut left_running = action != "restart";
    let mut seen_starting = false;

    loop {
        sleep(STATE_POLL).await;
        let current = state(ctx, server).await;
        let current = current.as_deref().unwrap_or("unknown");
        let seconds = started.elapsed().as_secs();

        if current != "running" {
            left_running = true;
        }
        if current == "starting" {
            seen_starting = true;
        }

        if current == target && left_running {
            return Ok(format!(
                "Sent {action} to server {server}. It is {target} after {seconds} s."
            ));
        }
        if target == "running" && current == "offline" && seen_starting {
            return Err(format!(
                "Sent {action} to server {server}, but it stopped again while starting, so it probably crashed. Check read_console."
            ));
        }
        if started.elapsed() >= Duration::from_secs(timeout) {
            return Err(format!(
                "Sent {action} to server {server}, but it is still {current} after {timeout} s (waiting for {target})."
            ));
        }
    }
}

async fn send(ctx: &Ctx, server: &str, command: &str) -> Result<(), String> {
    if command.chars().count() > 1024 {
        return Err("`command` must be at most 1024 characters".into());
    }

    if let Err(err) = request(
        ctx,
        Method::POST,
        &format!("/api/client/servers/{server}/command"),
        None,
        Some(&json!({ "command": command })),
    )
    .await
    {
        return Err(match state(ctx, server).await.as_deref() {
            Some("offline") => {
                format!("Server {server} is offline. Start it with power_server first.")
            }
            _ => err,
        });
    }
    Ok(())
}

pub async fn command(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let command = text(args, "command")?;
    send(ctx, server, command).await?;

    Ok(format!(
        "Sent \"{command}\" to server {server}. Use read_console to see the output, or run_command to send a command and wait for its output in one step."
    ))
}

/// The console's last lines, without colour codes.
async fn console_lines(ctx: &Ctx, server: &str, lines: i64) -> Result<Vec<String>, String> {
    let query = to_map(json!({ "lines": lines }));
    let reply = proxy::call(
        ctx,
        Method::GET,
        &format!("/api/client/servers/{server}/logs"),
        Some(&query),
        None,
    )
    .await?;
    if reply.status >= 400 {
        return Err(super::output::render(&reply).err().unwrap_or_default());
    }

    Ok(strip_ansi(&String::from_utf8_lossy(&reply.body))
        .lines()
        .map(str::to_string)
        .collect())
}

/// The lines of `now` that came after `before`. Both are tails of the same
/// growing log, so `now` is a suffix of `before` followed by the new lines;
/// the longest such overlap wins. Without any overlap everything is new.
fn new_lines(before: &[String], now: &[String]) -> Vec<String> {
    for skip in 0..before.len() {
        let rest = &before[skip..];
        if now.starts_with(rest) {
            return now[rest.len()..].to_vec();
        }
    }
    now.to_vec()
}

enum Matcher<'a> {
    Text(&'a str),
    Regex(regex::Regex),
}

impl Matcher<'_> {
    fn matches(&self, output: &str) -> bool {
        match self {
            Self::Text(text) => output.contains(text),
            Self::Regex(regex) => regex.is_match(output),
        }
    }
}

pub async fn run_command(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let command = text(args, "command")?;
    let wait_for = optional_text(args, "wait_for")?.filter(|pattern| !pattern.is_empty());
    let timeout = Duration::from_secs(integer(args, "timeout_seconds", 10, 1, 60)? as u64);
    let matcher = match (wait_for, boolean(args, "regex")?) {
        (Some(pattern), true) => Some(Matcher::Regex(
            regex::RegexBuilder::new(pattern)
                .size_limit(1 << 20)
                .build()
                .map_err(|err| format!("`wait_for` is not a valid regular expression: {err}"))?,
        )),
        (Some(pattern), false) => Some(Matcher::Text(pattern)),
        (None, true) => return Err("`regex` only applies together with `wait_for`".into()),
        (None, false) => None,
    };

    let before = console_lines(ctx, server, MAX_CONSOLE_LINES).await?;
    send(ctx, server, command).await?;

    let started = Instant::now();
    let mut output: Vec<String> = Vec::new();
    let mut last_change = started;
    let matched = loop {
        sleep(CONSOLE_POLL).await;
        let now = console_lines(ctx, server, MAX_CONSOLE_LINES).await?;
        let new = new_lines(&before, &now);
        if new != output {
            output = new;
            last_change = Instant::now();
        }

        match &matcher {
            Some(matcher) if matcher.matches(&output.join("\n")) => break true,
            None if !output.is_empty() && last_change.elapsed() >= QUIET => break true,
            _ => {}
        }
        if started.elapsed() >= timeout {
            break false;
        }
    };

    let seconds = started.elapsed().as_secs_f32();
    let body = if output.is_empty() {
        "(no new console output)".to_string()
    } else {
        output.join("\n")
    };
    let text = match (wait_for, matched) {
        (Some(pattern), true) => format!(
            "Ran \"{command}\" on server {server}; \"{pattern}\" appeared after {seconds:.1} s. Console output since the command:\n{body}"
        ),
        (Some(pattern), false) => {
            return Err(truncate(format!(
                "Ran \"{command}\" on server {server}, but \"{pattern}\" did not appear within {} s. Console output since the command:\n{body}",
                timeout.as_secs()
            )));
        }
        (None, _) => format!(
            "Ran \"{command}\" on server {server}. Console output since the command ({seconds:.1} s):\n{body}"
        ),
    };
    Ok(truncate(text))
}

pub async fn console(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let lines = integer(args, "lines", 100, 1, MAX_CONSOLE_LINES)?;
    let log = console_lines(ctx, server, lines).await?.join("\n");

    let output = tail(&log, lines as usize);
    Ok(if output.trim().is_empty() {
        "The console has no output yet.".into()
    } else {
        truncate(output)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn new_lines_follow_the_old_tail() {
        assert_eq!(new_lines(&lines("a\nb"), &lines("a\nb\nc")), lines("c"));
        assert_eq!(new_lines(&lines("a\nb"), &lines("a\nb")), Vec::<String>::new());
        // The window moved on: "a" scrolled out.
        assert_eq!(new_lines(&lines("a\nb\nc"), &lines("b\nc\nd\ne")), lines("d\ne"));
        // Nothing in common, for example after a restart cleared the log.
        assert_eq!(new_lines(&lines("a\nb"), &lines("x\ny")), lines("x\ny"));
        assert_eq!(new_lines(&[], &lines("x")), lines("x"));
    }
}
