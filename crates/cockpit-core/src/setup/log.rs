//! The output of a long task (image build) in the assistant: collapsed to the last lines, on
//! request complete.

/// How many lines the collapsed output shows.
pub const LOG_TAIL_LINES: usize = 5;

/// The lines to show: all of them when `expanded`, otherwise only the last [`LOG_TAIL_LINES`].
pub fn log_visible(lines: &[String], expanded: bool) -> String {
    let from = if expanded {
        0
    } else {
        lines.len().saturating_sub(LOG_TAIL_LINES)
    };
    lines[from..].join("\n")
}

/// How many lines the collapsed output hides.
pub fn log_hidden(lines: &[String]) -> usize {
    lines.len().saturating_sub(LOG_TAIL_LINES)
}
