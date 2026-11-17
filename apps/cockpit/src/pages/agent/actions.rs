//! What the buttons of the agent page do: each one dispatches an action to the store.

use cockpit_core::Action;
use leptos::prelude::*;

use super::context::AgentCtx;

impl AgentCtx {
    /// Shows the message of the last action that did not work (or clears it).
    fn note_result(self, result: Result<cockpit_core::Applied, cockpit_core::ActionError>) {
        self.problem.set(result.err().map(|e| e.message()));
    }

    pub(super) fn start(self) {
        let options = self.form.chosen();
        self.note_result(self.store.dispatch(Action::StartAgent {
            ticket: self.key.get_value(),
            options,
        }));
    }

    pub(super) fn cancel(self) {
        let _ = self.store.dispatch(Action::CancelAgent {
            ticket: self.key.get_value(),
        });
    }

    pub(super) fn approve(self) {
        self.note_result(self.store.dispatch(Action::ApprovePlan {
            ticket: self.key.get_value(),
        }));
        self.note.set(String::new());
    }

    /// Resume a halted run after the limit was raised.
    pub(super) fn raise(self, extra: u64) {
        self.note_result(self.store.dispatch(Action::ResumeStopped {
            run: self.key.get_value(),
            extra,
        }));
    }

    pub(super) fn reject(self) {
        let _ = self.store.dispatch(Action::RejectPlan {
            ticket: self.key.get_value(),
        });
        self.note.set(String::new());
    }

    /// Answer to a question of the agent.
    pub(super) fn send_answer(self, text: String) {
        let sent = self.store.dispatch(Action::AnswerAgent {
            run: self.key.get_value(),
            text,
        });
        if sent.is_ok() {
            self.answer.set(String::new());
        }
    }

    /// Hint to the running agent.
    pub(super) fn send_steer(self) {
        let sent = self.store.dispatch(Action::SteerAgent {
            run: self.key.get_value(),
            text: self.steer.get_untracked(),
        });
        if sent.is_ok() {
            self.steer.set(String::new());
        }
    }

    pub(super) fn pause(self) {
        let _ = self.store.dispatch(Action::PauseAgent {
            run: self.key.get_value(),
        });
    }

    pub(super) fn resume(self) {
        self.note_result(self.store.dispatch(Action::ResumeAgent {
            run: self.key.get_value(),
        }));
    }

    /// Asks for a new plan with the note.
    pub(super) fn revise(self) {
        let text = self.note.get_untracked();
        let revised = self.store.dispatch(Action::RevisePlan {
            ticket: self.key.get_value(),
            note: text,
        });
        if revised.is_ok() {
            self.note.set(String::new());
        }
    }

    pub(super) fn to_ticket(self) {
        if let Some(item) = self.item.get_untracked() {
            self.store.open_item(&item.id);
        }
    }

    pub(super) fn apply_fix(self) {
        let _ = self.store.dispatch(Action::ApplyFix {
            run: self.key.get_value(),
        });
    }

    pub(super) fn discard_fix(self) {
        let _ = self.store.dispatch(Action::DiscardFix {
            run: self.key.get_value(),
        });
    }
}
