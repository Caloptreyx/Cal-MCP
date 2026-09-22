use super::{Index, Operation};

const STOP_WORDS: [&str; 8] = ["api", "the", "a", "an", "of", "for", "to", "and"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    Client,
    Admin,
}

impl Scope {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("all") => Ok(Self::All),
            Some("client") => Ok(Self::Client),
            Some("admin") => Ok(Self::Admin),
            Some(other) => Err(format!(
                "`scope` must be all, client or admin, got \"{other}\""
            )),
        }
    }

    fn includes(self, path: &str) -> bool {
        match self {
            Self::All => true,
            Self::Client => path.starts_with("/api/client/"),
            Self::Admin => path.starts_with("/api/admin/"),
        }
    }
}

fn stem(word: &str) -> String {
    let word = word.to_ascii_lowercase();
    if word.len() > 4 && word.ends_with("ies") {
        format!("{}y", &word[..word.len() - 3])
    } else if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        word[..word.len() - 1].to_string()
    } else {
        word
    }
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut spaced = String::with_capacity(text.len());
    let mut previous_lower = false;
    for c in text.chars() {
        if c.is_ascii_uppercase() && previous_lower {
            spaced.push(' ');
        }
        previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        spaced.push(c);
    }

    spaced
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(stem)
        .filter(|word| !STOP_WORDS.contains(&word.as_str()))
        .collect()
}

fn method_intent(word: &str) -> &'static [&'static str] {
    match word {
        "list" | "get" | "show" | "view" | "read" | "fetch" | "find" | "info" | "detail" => {
            &["GET"]
        }
        "create" | "add" | "new" | "make" | "upload" | "send" | "run" | "execute" | "start" => {
            &["POST"]
        }
        "update" | "edit" | "change" | "set" | "rename" | "modify" => &["PUT", "PATCH", "POST"],
        "delete" | "remove" | "destroy" | "revoke" | "unlink" => &["DELETE"],
        _ => &[],
    }
}

fn score(operation: &Operation, terms: &[String], intents: &[&str]) -> usize {
    let mut score = 0;
    for term in terms {
        if operation.path_words.contains(term) {
            score += 4;
        } else if operation.field_words.contains(term)
            || (term.len() >= 3
                && operation
                    .path_words
                    .iter()
                    .chain(&operation.field_words)
                    .any(|word| {
                        word.len() >= 3
                            && (word.starts_with(term.as_str()) || term.starts_with(word.as_str()))
                    }))
        {
            score += 1;
        }
    }

    if score > 0 && intents.contains(&operation.method.as_str()) {
        score += 2;
    }
    score
}

impl Index {
    pub fn search(&self, query: &str, scope: Scope, limit: usize) -> Vec<&Operation> {
        let intents: Vec<&str> = query
            .split(|c: char| !c.is_ascii_alphanumeric())
            .flat_map(|word| method_intent(&word.to_ascii_lowercase()))
            .copied()
            .collect();
        let mut terms = tokenize(query);
        terms.dedup();

        let mut scored: Vec<(usize, &Operation)> = self
            .operations
            .iter()
            .filter(|operation| scope.includes(&operation.path))
            .map(|operation| (score(operation, &terms, &intents), operation))
            .filter(|(score, _)| *score > 0)
            .collect();

        scored.sort_by(|(a_score, a), (b_score, b)| {
            b_score
                .cmp(a_score)
                .then_with(|| a.path.len().cmp(&b.path.len()))
        });
        scored
            .into_iter()
            .take(limit)
            .map(|(_, operation)| operation)
            .collect()
    }
}
