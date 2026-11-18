//! Small message builders shared by the steps, the plan and the log of a run.

use cockpit_i18n::{Msg, msg};

/// `1 file` / `3 files`.
pub(super) fn files_msg(n: usize) -> Msg {
    msg!(CoreAgentFiles, n = n)
}

/// `1 comment` / `3 comments`.
pub(super) fn comments_msg(n: usize) -> Msg {
    msg!(CoreAgentComments, n = n)
}

/// A line of the activity log: the text behind a bullet.
pub(super) fn bullet(text: Msg) -> Msg {
    msg!(CoreAgentLogLine, text = text)
}

/// Technical text (a command, a name) that reads the same in every language.
pub(super) fn raw(text: impl Into<String>) -> Msg {
    Msg::raw(text)
}
