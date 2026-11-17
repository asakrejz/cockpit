//! Own and others' line comments of a review: create, edit, take over suggestions, show.

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

    /// Comments that are not implemented yet – own as well as others'.
    pub fn open_comments(&self) -> Vec<&LineComment> {
        self.line_comments.iter().filter(|c| c.is_open()).collect()
    }

    /// Sets the comments to „done“, with a reference to the run. Returns how
    /// many there were; already done and unknown ones do not count. This changes nothing about
    /// the verdict, but the review then counts as to be sent anew.
    pub fn resolve_comments(&mut self, ids: &[String], run: &str) -> usize {
        let mut n = 0;
        for c in self
            .line_comments
            .iter_mut()
            .filter(|c| c.is_open() && ids.contains(&c.id))
        {
            c.resolved = Some(run.to_string());
            n += 1;
        }
        n
    }

    /// Takes over the change suggestion of a comment into the diff: the line is replaced by
    /// the replacement. An unchanged line stays as a removed one. Comments
    /// further down in the file move along. `false` if there is nothing to take over.
    pub fn apply_comment_suggestion(&mut self, id: &str) -> bool {
        let Some((file, index, code)) = self
            .line_comments
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| Some((c.file.clone(), c.line, c.suggestion.clone()?)))
        else {
            return false;
        };
        let Some(diff) = self.files.iter_mut().find(|f| f.name == file) else {
            return false;
        };
        let Some(old) = diff.lines.get(index).cloned() else {
            return false;
        };
        if old.kind == LineKind::Del {
            return false;
        }
        let mut replacement = Vec::new();
        if old.kind == LineKind::Context {
            replacement.push(DiffLine {
                kind: LineKind::Del,
                ..old.clone()
            });
        }
        replacement.extend(code.lines().enumerate().map(|(i, text)| DiffLine {
            no: old.no + i as u32,
            kind: LineKind::Add,
            text: text.to_string(),
        }));
        let grown = replacement.len() - 1;
        diff.lines.splice(index..=index, replacement);
        self.shift_comments(&file, index, grown as isize);
        true
    }

    /// Moves the comments below `index` by `by` lines.
    pub(super) fn shift_comments(&mut self, file: &str, index: usize, by: isize) {
        for c in self
            .line_comments
            .iter_mut()
            .filter(|c| c.file == file && c.line > index)
        {
            c.line = c.line.saturating_add_signed(by);
        }
    }

    /// Applies the change of a diagnosis: the line is replaced or removed.
    /// `false` if file or line (no longer) fit.
    pub fn apply_fix(&mut self, fix: &Fix) -> bool {
        let Some(diff) = self.files.iter_mut().find(|f| f.name == fix.file) else {
            return false;
        };
        let Some(index) = diff.lines.iter().position(|l| {
            l.no == fix.no && l.kind != LineKind::Del && l.text.trim_end() == fix.before.trim_end()
        }) else {
            return false;
        };
        let file = fix.file.clone();
        match &fix.after {
            Some(after) => diff.lines[index].text = after.clone(),
            None => {
                diff.lines.remove(index);
                self.shift_comments(&file, index, -1);
            }
        }
        true
    }

    /// Determines how the next restart of a check ends (after a fix: green).
    pub fn set_rerun(&mut self, check: &str, rerun: Checks) {
        if let Some(c) = self.check_runs.iter_mut().find(|c| c.name == check) {
            c.rerun = rerun;
        }
    }

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
