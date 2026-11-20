use super::{
    Ctx, Output,
    args::{any_text, boolean, integer, optional_integer, optional_text, server, text, to_map},
    output::{MAX_TEXT, floor_boundary, render, request, request_json, sha256_hex},
};
use crate::proxy;
use axum::http::Method;
use serde_json::{Map, Value, json};

fn contents_path(server: &str) -> String {
    format!("/api/client/servers/{server}/files/contents")
}

/// Reads a whole text file through the panel. `Ok(None)` when it does not exist.
async fn fetch(ctx: &Ctx, server: &str, file: &str) -> Result<Option<String>, String> {
    let query = to_map(json!({ "file": file }));
    let reply = proxy::call(ctx, Method::GET, &contents_path(server), Some(&query), None).await?;

    match reply.status {
        404 => return Ok(None),
        413 => {
            return Err(format!(
                "{file} is larger than the panel lets the file manager open"
            ));
        }
        status if status >= 400 => return Err(render(&reply).err().unwrap_or_default()),
        _ => {}
    }

    if reply.body.contains(&0) {
        return Err(format!("{file} is a binary file, not text"));
    }
    String::from_utf8(reply.body)
        .map(Some)
        .map_err(|_| format!("{file} is not a UTF-8 text file"))
}

async fn put(ctx: &Ctx, server: &str, file: &str, content: &str) -> Result<(), String> {
    let query = to_map(json!({ "file": file }));
    request(
        ctx,
        Method::POST,
        &format!("/api/client/servers/{server}/files/write"),
        Some(&query),
        Some(&Value::String(content.to_string())),
    )
    .await
    .map(drop)
}

/// Fails when the file is not the version the caller read.
fn check_expected(file: &str, content: &str, expected: Option<&str>) -> Result<(), String> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = sha256_hex(content.as_bytes());
    if actual.eq_ignore_ascii_case(expected.trim()) {
        return Ok(());
    }
    Err(format!(
        "{file} changed since it was read: its sha256 is now {actual} ({} bytes), not {expected}. Read it again before changing it.",
        content.len()
    ))
}

pub async fn list(ctx: &Ctx, args: &Map<String, Value>) -> Result<Output, String> {
    let server = server(args)?;
    let directory = optional_text(args, "directory")?
        .filter(|directory| !directory.is_empty())
        .unwrap_or("/");
    let query = to_map(json!({
        "directory": directory,
        "page": integer(args, "page", 1, 1, i64::MAX)?,
        "per_page": integer(args, "per_page", 100, 1, 100)?,
    }));

    let (text, json) = request_json(
        ctx,
        Method::GET,
        &format!("/api/client/servers/{server}/files/list"),
        Some(&query),
        None,
    )
    .await?;
    Ok(Output::structured(text, json))
}

enum Range {
    Lines { start: usize, end: Option<usize> },
    Bytes { offset: usize, length: Option<usize> },
}

impl Range {
    fn parse(args: &Map<String, Value>) -> Result<Self, String> {
        let start = optional_integer(args, "start_line", 1, i64::MAX)?;
        let end = optional_integer(args, "end_line", 1, i64::MAX)?;
        let offset = optional_integer(args, "offset", 0, i64::MAX)?;
        let length = optional_integer(args, "length", 1, i64::MAX)?;

        if (start.is_some() || end.is_some()) && (offset.is_some() || length.is_some()) {
            return Err("Use either start_line/end_line or offset/length, not both".into());
        }
        if let (Some(start), Some(end)) = (start, end)
            && end < start
        {
            return Err("`end_line` must not be before `start_line`".into());
        }

        Ok(if offset.is_some() || length.is_some() {
            Self::Bytes {
                offset: offset.unwrap_or(0) as usize,
                length: length.map(|length| length as usize),
            }
        } else {
            Self::Lines {
                start: start.unwrap_or(1) as usize,
                end: end.map(|end| end as usize),
            }
        })
    }
}

/// Picks the requested part of a file, never more than [`MAX_TEXT`] bytes, and
/// describes what was shown.
fn view(file: &str, content: &str, range: Range) -> Result<Output, String> {
    let total = content.len();
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let hash = sha256_hex(content.as_bytes());

    // Byte range of the slice, plus the line range when reading by lines.
    let (from, to, shown_lines) = match range {
        Range::Lines { start, end } => {
            if start > lines.len().max(1) {
                return Err(format!(
                    "`start_line` {start} is past the end of {file}, which has {} lines",
                    lines.len()
                ));
            }
            let last = end.unwrap_or(usize::MAX).min(lines.len());
            let from: usize = lines[..start - 1].iter().map(|line| line.len()).sum();
            let mut to = from;
            let mut shown = 0;
            for line in lines.iter().take(last).skip(start - 1) {
                if to + line.len() - from > MAX_TEXT {
                    break;
                }
                to += line.len();
                shown += 1;
            }
            if shown == 0 && start <= last {
                // One line longer than a whole result: show its start.
                to = from + floor_boundary(&content[from..], MAX_TEXT);
            }
            (from, to, Some((start, start - 1 + shown)))
        }
        Range::Bytes { offset, length } => {
            if offset > total {
                return Err(format!(
                    "`offset` {offset} is past the end of {file}, which is {total} bytes"
                ));
            }
            let mut from = offset;
            while !content.is_char_boundary(from) {
                from += 1;
            }
            let wanted = length.unwrap_or(usize::MAX).min(MAX_TEXT);
            let to = from + floor_boundary(&content[from..], wanted);
            (from, to, None)
        }
    };

    let whole = from == 0 && to == total;
    let note = if whole {
        format!(
            "[{file}: the whole file, {total} bytes, {} lines, sha256 {hash}]",
            lines.len()
        )
    } else {
        let shown = match shown_lines {
            Some((first, last)) if last >= first => format!("lines {first}-{last} of {}", lines.len()),
            Some(_) => format!("part of line {} (it is very long)", from_line(&lines, from)),
            None => format!("bytes {from}-{to} of {total}"),
        };
        let next = match shown_lines {
            Some((first, last)) if to < total && last >= first => {
                format!(" Read on with start_line={}.", last + 1)
            }
            _ if to < total => format!(" Read on with offset={to}."),
            _ => String::new(),
        };
        format!(
            "[{file}: showing {shown} ({total} bytes in all), sha256 of the whole file {hash}. \
This is only part of the file: do not pass it to write_file, which replaces the whole file and would lose the rest. \
Change it with edit_file instead.{next}]"
        )
    };

    Ok(Output {
        text: content[from..to].to_string(),
        note: Some(note),
        structured: None,
    })
}

/// The one-based line that contains byte `offset`.
fn from_line(lines: &[&str], offset: usize) -> usize {
    let mut seen = 0;
    for (index, line) in lines.iter().enumerate() {
        seen += line.len();
        if offset < seen {
            return index + 1;
        }
    }
    lines.len()
}

pub async fn read(ctx: &Ctx, args: &Map<String, Value>) -> Result<Output, String> {
    let server = server(args)?;
    let file = text(args, "file")?;
    let range = Range::parse(args)?;
    let content = fetch(ctx, server, file)
        .await?
        .ok_or_else(|| format!("{file} does not exist on server {server}"))?;

    view(file, &content, range)
}

pub async fn write(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let file = text(args, "file")?;
    let content = any_text(args, "content")?;
    let expected = optional_text(args, "expected_sha256")?;

    match fetch(ctx, server, file).await {
        Ok(Some(current)) => {
            check_expected(file, &current, expected)?;
            if expected.is_none() && current.len() > MAX_TEXT {
                return Err(format!(
                    "{file} is {} bytes, more than read_file returns at once, so replacing it with write_file would likely drop the part you have not seen. \
Use edit_file to change part of it. If you really mean to replace all of it, pass expected_sha256 from read_file.",
                    current.len()
                ));
            }
        }
        Ok(None) if expected.is_some() => {
            return Err(format!(
                "{file} does not exist, but expected_sha256 was given. Leave expected_sha256 out to create it."
            ));
        }
        Ok(None) => {}
        // A file that cannot be read cannot have been read in part either.
        Err(err) if expected.is_some() => return Err(err),
        Err(_) => {}
    }

    put(ctx, server, file, content).await?;
    Ok(format!(
        "Wrote {} bytes to {file} on server {server}. Its sha256 is now {}.",
        content.len(),
        sha256_hex(content.as_bytes())
    ))
}

pub async fn edit(ctx: &Ctx, args: &Map<String, Value>) -> Result<String, String> {
    let server = server(args)?;
    let file = text(args, "file")?;
    let old = text(args, "old_string")?;
    let new = any_text(args, "new_string")?;
    let replace_all = boolean(args, "replace_all")?;
    let expected = optional_text(args, "expected_sha256")?;

    if old == new {
        return Err("`old_string` and `new_string` are the same, so there is nothing to change".into());
    }

    let content = fetch(ctx, server, file)
        .await?
        .ok_or_else(|| format!("{file} does not exist on server {server}. Create it with write_file."))?;
    check_expected(file, &content, expected)?;

    // Files with Windows line endings: let `\n` in the arguments match `\r\n`.
    let crlf = |text: &str| text.replace("\r\n", "\n").replace('\n', "\r\n");
    let (old, new) = if !content.contains(old)
        && old.contains('\n')
        && content.contains("\r\n")
        && content.contains(&crlf(old))
    {
        (crlf(old), crlf(new))
    } else {
        (old.to_string(), new.to_string())
    };

    let count = content.matches(old.as_str()).count();
    if count == 0 {
        return Err(format!(
            "`old_string` was not found in {file}. It has to match the file exactly, including spaces, indentation and line breaks. Read the file again and copy the text from it."
        ));
    }
    if count > 1 && !replace_all {
        return Err(format!(
            "`old_string` appears {count} times in {file}. Add surrounding lines so it matches exactly once, or set replace_all to replace every occurrence."
        ));
    }

    let updated = if replace_all {
        content.replace(old.as_str(), &new)
    } else {
        content.replacen(old.as_str(), &new, 1)
    };
    put(ctx, server, file, &updated).await?;

    Ok(format!(
        "Replaced {count} occurrence{} in {file} on server {server}. It is now {} bytes, sha256 {}.",
        if count == 1 { "" } else { "s" },
        updated.len(),
        sha256_hex(updated.as_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(range: Range, content: &str) -> Output {
        view("/f.txt", content, range).unwrap()
    }

    #[test]
    fn small_files_are_shown_whole_with_their_hash() {
        let output = lines(Range::Lines { start: 1, end: None }, "a\nb\n");
        assert_eq!(output.text, "a\nb\n");
        let note = output.note.unwrap();
        assert!(note.contains("the whole file, 4 bytes, 2 lines"), "{note}");
        assert!(note.contains(&sha256_hex(b"a\nb\n")), "{note}");
    }

    #[test]
    fn line_ranges_are_one_based_and_inclusive() {
        let output = lines(Range::Lines { start: 2, end: Some(3) }, "a\nb\nc\nd");
        assert_eq!(output.text, "b\nc\n");
        let note = output.note.unwrap();
        assert!(note.contains("lines 2-3 of 4"), "{note}");
        assert!(note.contains("start_line=4"), "{note}");
        assert!(note.contains("edit_file"), "{note}");
    }

    #[test]
    fn large_files_stop_at_a_line_boundary_and_say_so() {
        let line = format!("{}\n", "x".repeat(99));
        let content = line.repeat(2000); // 200 000 bytes
        let output = lines(Range::Lines { start: 1, end: None }, &content);
        assert_eq!(output.text.len(), MAX_TEXT);
        assert!(output.text.ends_with('\n'));
        let note = output.note.unwrap();
        assert!(note.contains("lines 1-1000 of 2000"), "{note}");
        assert!(note.contains("do not pass it to write_file"), "{note}");
    }

    #[test]
    fn byte_ranges_respect_character_boundaries() {
        let output = lines(
            Range::Bytes {
                offset: 1,
                length: Some(2),
            },
            "éa",
        );
        // Offset 1 is inside "é", so the slice starts after it.
        assert_eq!(output.text, "a");
        assert!(output.note.unwrap().contains("bytes 2-3 of 3"));
    }

    #[test]
    fn ranges_past_the_end_are_errors() {
        assert!(view("/f", "a\n", Range::Lines { start: 5, end: None }).is_err());
        assert!(
            view(
                "/f",
                "a",
                Range::Bytes {
                    offset: 9,
                    length: None
                }
            )
            .is_err()
        );
    }

    #[test]
    fn a_very_long_line_is_cut() {
        let content = "y".repeat(MAX_TEXT + 10);
        let output = lines(Range::Lines { start: 1, end: None }, &content);
        assert_eq!(output.text.len(), MAX_TEXT);
        let note = output.note.unwrap();
        assert!(note.contains("part of line 1"), "{note}");
        assert!(note.contains(&format!("offset={MAX_TEXT}")), "{note}");
    }
}
