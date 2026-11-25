use super::texts::{bullet, raw};
use super::*;
use cockpit_i18n::{Lang, Msg, msg};

impl AgentRun {
    /// The log of a run as messages: what the agent and you said, plus the consumption.
    pub fn log_msgs(&self) -> Vec<Msg> {
        let mut lines: Vec<Msg> = self
            .messages
            .iter()
            .map(|m| {
                let text = m.msg.clone().unwrap_or_else(|| raw(m.text.clone()));
                bullet(match m.from {
                    Speaker::Agent => msg!(CoreAgentLogFromAgent, text = text),
                    Speaker::User => msg!(CoreAgentLogFromUser, text = text),
                })
            })
            .collect();
        let usage = self.usage();
        if !usage.is_empty() {
            lines.push(bullet(msg!(CoreAgentLogUsage, usage = usage.line_msg())));
        }
        if lines.is_empty() {
            return vec![msg!(CoreAgentLogNone)];
        }
        lines
    }

    /// The log in `lang`, one line per entry.
    pub fn log_in(&self, lang: Lang) -> String {
        self.log_msgs()
            .iter()
            .map(|m| m.render(lang))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
