use super::*;

/// This many ticks a restarted check runs in the demo.
pub const CHECK_STEPS: u8 = 3;

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
    /// How a restart ends in the demo. Fixed so that it stays traceable:
    /// a real error stays red, a flaky test turns green.
    #[serde(default = "passed")]
    pub rerun: Checks,
    /// Ticks a running check still needs.
    #[serde(default)]
    pub remaining: u8,
}

fn passed() -> Checks {
    Checks::Passed
}

impl CheckRun {
    pub fn new(name: &str, status: Checks, duration_s: u32, log: &str) -> Self {
        Self {
            name: name.into(),
            status,
            duration_s,
            log: log.into(),
            rerun: Checks::Passed,
            remaining: 0,
        }
    }

    /// Determines how a restart ends.
    pub fn rerun_as(mut self, rerun: Checks) -> Self {
        self.rerun = rerun;
        self
    }

    /// `2:05 Min.` or `42 Sek.`; while running `läuft`.
    pub fn duration_label(&self) -> String {
        if self.status == Checks::Running {
            return "läuft".into();
        }
        match self.duration_s {
            s if s < 60 => format!("{s} Sek."),
            s => format!("{}:{:02} Min.", s / 60, s % 60),
        }
    }

    /// Only a red check can be restarted.
    pub(super) fn restart(&mut self) -> bool {
        if self.status != Checks::Failed {
            return false;
        }
        self.status = Checks::Running;
        self.remaining = CHECK_STEPS;
        self.log = format!("$ {} (Neustart)\nwird ausgeführt …\n", self.name);
        true
    }

    /// One tick. `true` if the check finished in the process.
    pub(super) fn advance(&mut self) -> bool {
        if self.status != Checks::Running {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining > 0 {
            return false;
        }
        self.status = self.rerun;
        self.log = match self.rerun {
            Checks::Failed => format!(
                "$ {} (Neustart)\nerror: derselbe Fehler wie zuvor – der Check bleibt rot.\n",
                self.name
            ),
            _ => format!("$ {} (Neustart)\nalle Schritte erfolgreich\n", self.name),
        };
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

    /// Lets every running check advance one step. Returns the names of the checks
    /// that finished in the process.
    pub fn advance_checks(&mut self) -> Vec<String> {
        let done: Vec<String> = self
            .check_runs
            .iter_mut()
            .filter_map(|c| c.advance().then(|| c.name.clone()))
            .collect();
        if !done.is_empty() {
            self.sync_checks();
        }
        done
    }

    pub fn running_checks(&self) -> usize {
        self.check_runs
            .iter()
            .filter(|c| c.status == Checks::Running)
            .count()
    }
}
