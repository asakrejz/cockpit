use super::*;

/// A single check of a pull request (build, lint, tests …).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CheckRun {
    pub name: String,
    pub status: Checks,
    /// Duration of the last run in seconds.
    #[serde(default)]
    pub duration_s: u32,
    /// Output of the last run.
    #[serde(default)]
    pub log: String,
}

impl CheckRun {
    pub fn new(name: &str, status: Checks, duration_s: u32, log: &str) -> Self {
        Self {
            name: name.into(),
            status,
            duration_s,
            log: log.into(),
        }
    }

    /// `2:05 Min.` or `42 Sek.`; while running `läuft`.
    pub fn duration_msg(&self) -> cockpit_i18n::Msg {
        use cockpit_i18n::msg;
        if self.status == Checks::Running {
            return msg!(CoreReviewDurationRunning);
        }
        match self.duration_s {
            s if s < 60 => msg!(CoreReviewDurationSeconds, n = s),
            s => msg!(
                CoreReviewDurationMinutes,
                m = s / 60,
                s = format!("{:02}", s % 60)
            ),
        }
    }

    /// Only a red check can be restarted. It then runs again; the next sync brings its result
    /// and its output.
    pub(super) fn restart(&mut self) -> bool {
        if self.status != Checks::Failed {
            return false;
        }
        self.status = Checks::Running;
        self.log.clear();
        true
    }
}

impl Review {
    /// Overall state from the individual checks: one red is enough for „failed“,
    /// otherwise „running“ applies as long as one is running. Without details `checks` stays as it is.
    pub fn sync_checks(&mut self) {
        if self.check_runs.is_empty() {
            return;
        }
        let any = |c: Checks| self.check_runs.iter().any(|r| r.status == c);
        self.checks = if any(Checks::Failed) {
            Checks::Failed
        } else if any(Checks::Running) {
            Checks::Running
        } else {
            Checks::Passed
        };
    }

    /// Restarts a failed check. `false` if it does not exist or is
    /// not red.
    pub fn restart_check(&mut self, name: &str) -> bool {
        let restarted = self
            .check_runs
            .iter_mut()
            .find(|c| c.name == name)
            .is_some_and(CheckRun::restart);
        if restarted {
            self.sync_checks();
        }
        restarted
    }
}
