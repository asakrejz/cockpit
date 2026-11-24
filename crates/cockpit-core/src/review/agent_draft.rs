//! Drafts of the agent: checked against the diff, kept as suggestions that wait for your
//! decision. Nothing here talks to GitHub.

use super::*;

/// At most this many drafts per review run; the rest is dropped.
pub const MAX_AGENT_COMMENTS: usize = 40;

/// Longest range of lines a draft may cover.
const MAX_RANGE: u32 = 60;

/// A comment as the agent reported it, before it is checked against the diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentComment {
    pub path: String,
    pub line: u32,
    pub start_line: Option<u32>,
    pub side: DiffSide,
    pub text: String,
    /// Replacement code for the line(s).
    pub suggestion: Option<String>,
}

impl Review {
    /// Does `side` of file `path` have a line with this number?
    pub fn has_line(&self, path: &str, side: DiffSide, line: u32) -> bool {
        line > 0
            && self
                .files
                .iter()
                .any(|f| f.name == path && f.lines.iter().any(|l| l.no == line && side.has(l.kind)))
    }

    /// Number of drafts of the agent.
    pub fn agent_drafts(&self) -> usize {
        self.suggestions.len()
    }

    /// A new review run starts: the old suggestions and the old summary are replaced.
    pub fn begin_agent_review(&mut self) {
        self.suggestions.clear();
        self.agent_summary.clear();
        self.agent_reviewed = false;
        self.sent = false;
    }

    /// Takes a comment of the agent as a draft. `false` if it does not fit the diff (unknown
    /// file, no such line on that side, empty text, duplicate, too many) – then nothing changes.
    pub fn add_agent_comment(&mut self, c: AgentComment) -> bool {
        let text = c.text.trim().to_string();
        let code = if c.side == DiffSide::Right {
            clean_suggestion(c.suggestion.as_deref())
        } else {
            None
        };
        if (text.is_empty() && code.is_none())
            || self.agent_drafts() >= MAX_AGENT_COMMENTS
            || !self.has_line(&c.path, c.side, c.line)
        {
            return false;
        }
        // A range only counts if it starts on the same side, before the last line and not too far away.
        let start = c.start_line.filter(|&s| {
            s < c.line && c.line - s <= MAX_RANGE && self.has_line(&c.path, c.side, s)
        });
        if self
            .suggestions
            .iter()
            .any(|s| s.path == c.path && s.line == c.line && s.side == c.side && s.text == text)
        {
            return false;
        }
        let mut n = self.suggestions.len() + 1;
        while self.suggestions.iter().any(|s| s.id == format!("a{n}")) {
            n += 1;
        }
        let mut s = Suggestion::new(
            &format!("a{n}"),
            &format!("{} · Zeile {}", c.path, c.line),
            Level::Note,
            &text,
        );
        s.path = c.path;
        s.line = c.line;
        s.start_line = start;
        s.side = c.side;
        s.code = code;
        self.suggestions.push(s);
        self.sent = false;
        true
    }

    /// Stores the agent's own summary (limited, local only).
    pub fn set_agent_summary(&mut self, text: &str) {
        self.agent_summary = text.trim().chars().take(4_000).collect();
    }

    /// The run is done: the drafts wait for your decision.
    pub fn finish_agent_review(&mut self) -> usize {
        self.agent_reviewed = true;
        self.sent = false;
        self.suggestions.len()
    }

    /// Drafts that sit on the diff line at `index` of file `file` (position in `FileDiff::lines`).
    pub fn suggestions_at(&self, file: &str, index: usize) -> Vec<&Suggestion> {
        let Some(l) = self
            .files
            .iter()
            .find(|f| f.name == file)
            .and_then(|f| f.lines.get(index))
        else {
            return Vec::new();
        };
        self.suggestions
            .iter()
            .filter(|s| s.path == file && s.line == l.no && s.line > 0 && s.side.has(l.kind))
            .collect()
    }
}

#[cfg(test)]
mod tests;
