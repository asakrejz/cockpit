//! Sending a review: which comments go out and the sending itself.

use super::*;

impl Review {
    /// Own line comments that go out on sending: others' are already on GitHub,
    /// implemented ones are done.
    pub(super) fn outgoing(&self) -> impl Iterator<Item = &LineComment> {
        self.line_comments
            .iter()
            .filter(|c| c.is_own() && c.is_open())
    }

    /// This many comments go out on sending: approved suggestions of the agent
    /// and the open own line comments.
    pub fn comment_count(&self) -> usize {
        self.approved() + self.outgoing().count()
    }

    pub fn can_send(&self) -> bool {
        !self.sent && self.undecided() == 0
    }

    pub fn send_label(&self) -> String {
        if self.sent {
            return "Gesendet".into();
        }
        let n = self.comment_count();
        format!(
            "Review senden · {n} {}",
            if n == 1 { "Kommentar" } else { "Kommentare" }
        )
    }

    /// Sends the review and returns the comments: first the approved ones of the
    /// agent, then the own line comments.
    pub fn send(&mut self) -> Result<Vec<String>, SendError> {
        if self.sent {
            return Err(SendError::AlreadySent);
        }
        if self.undecided() > 0 {
            return Err(SendError::Undecided(self.undecided()));
        }
        self.sent = true;
        let approved = self
            .suggestions
            .iter()
            .filter(|s| s.decision == Some(Decision::Approved))
            .map(|s| s.text.clone());
        let own = self.outgoing().map(LineComment::body);
        Ok(approved.chain(own).collect())
    }
}
