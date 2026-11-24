//! Own and others' line comments of a review: create, edit, delete, show.

use super::*;

impl Review {
    /// Creates a comment on the line `line` (position in the diff) of the file `file`.
    /// Optionally with replacement code for this line. Returns the ID.
    ///
    /// Rejected are: neither text nor suggestion, unknown file or line, a
    /// suggestion on a removed line (it no longer exists after the merge) and a
    /// suggestion that does not change the line. Like every change this makes a sent
    /// review open again.
    pub fn add_line_comment(
        &mut self,
        file: &str,
        line: usize,
        text: &str,
        suggestion: Option<&str>,
    ) -> Option<String> {
        let text = text.trim();
        let suggestion = clean_suggestion(suggestion);
        let diff_line = self
            .files
            .iter()
            .find(|f| f.name == file)
            .and_then(|f| f.lines.get(line))?;
        if (text.is_empty() && suggestion.is_none())
            || !Self::suggestion_fits(diff_line, suggestion.as_deref())
        {
            return None;
        }
        let next = self
            .line_comments
            .iter()
            .filter_map(|c| c.id.strip_prefix('c')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let id = format!("c{next}");
        self.line_comments.push(LineComment {
            id: id.clone(),
            file: file.into(),
            line,
            text: text.into(),
            suggestion,
            author: None,
            resolved: None,
        });
        self.sent = false;
        Some(id)
    }

    pub(super) fn suggestion_fits(line: &DiffLine, suggestion: Option<&str>) -> bool {
        match suggestion {
            None => true,
            Some(code) => line.kind != LineKind::Del && code != line.text,
        }
    }

    /// Changes text and suggestion of an own comment (`None` removes the suggestion).
    /// The same rules apply as when creating.
    pub fn edit_line_comment(&mut self, id: &str, text: &str, suggestion: Option<&str>) -> bool {
        let text = text.trim();
        let suggestion = clean_suggestion(suggestion);
        let Some(index) = self
            .line_comments
            .iter()
            .position(|c| c.id == id && c.is_own())
        else {
            return false;
        };
        let comment = &self.line_comments[index];
        let fits = self
            .files
            .iter()
            .find(|f| f.name == comment.file)
            .and_then(|f| f.lines.get(comment.line))
            .is_some_and(|l| Self::suggestion_fits(l, suggestion.as_deref()));
        if (text.is_empty() && suggestion.is_none()) || !fits {
            return false;
        }
        let c = &mut self.line_comments[index];
        c.text = text.to_string();
        c.suggestion = suggestion;
        self.sent = false;
        true
    }

    pub fn delete_line_comment(&mut self, id: &str) -> bool {
        let before = self.line_comments.len();
        self.line_comments.retain(|c| c.id != id || !c.is_own());
        let removed = self.line_comments.len() < before;
        if removed {
            self.sent = false;
        }
        removed
    }

    // ---------- Implementing comments (follow-up job for the agent) ----------

    /// Own comments on a line, in the order of creation.
    pub fn line_comments_at(&self, file: &str, line: usize) -> Vec<&LineComment> {
        self.line_comments
            .iter()
            .filter(|c| c.file == file && c.line == line)
            .collect()
    }

    /// File and line number of a comment, e.g. `a.css · Zeile 3`.
    pub fn line_location(&self, comment: &LineComment) -> Option<String> {
        let file = self.files.iter().find(|f| f.name == comment.file)?;
        let line = file.lines.get(comment.line)?;
        Some(format!("{} · Zeile {}", file.name, line.no))
    }

    /// Before/after of a change suggestion; `None` if the comment has none.
    pub fn suggestion_diff(&self, comment: &LineComment) -> Option<SuggestionDiff> {
        let code = comment.suggestion.as_ref()?;
        let line = self
            .files
            .iter()
            .find(|f| f.name == comment.file)?
            .lines
            .get(comment.line)?;
        Some(SuggestionDiff {
            no: line.no,
            before: line.text.clone(),
            after: code.lines().map(str::to_string).collect(),
        })
    }
}
