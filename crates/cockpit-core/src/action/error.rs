use super::*;

/// Why an action was not applied. The workspace then stays unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    UnknownItem(String),
    UnknownReview(PrKey),
    UnknownSuggestion(String),
    UnknownComment(String),
    /// No ticket with this key.
    UnknownTicket(String),
    /// Text must not be empty, or the file and line do not exist.
    InvalidInput,
    /// Pull requests have no status.
    NotATicket,
    BranchExists,
    /// Nothing changes (already done, same status …).
    Unchanged,
    /// Nothing is running that would allow it (no waiting plan, no active run, no agent).
    NothingToDo,
    Send(SendError),
    Agent(AgentError),
    /// No check with this name.
    UnknownCheck(String),
    /// The pull request is merged and can no longer be changed.
    Merged(PrKey),
    /// Linking ticket and pull request is not possible.
    Link(LinkError),
    /// This person does not exist in the team.
    UnknownPerson(String),
    /// Merging happens only by hand: neither the agent nor the system may do it.
    MergeNotAllowed(Actor),
    /// As many runs are already working as the limit allows (the number).
    ParallelLimit(u32),
    /// The plan touches files that the rules of the repo do not allow.
    PlanViolatesRules(Vec<PathViolation>),
    /// Even with the raise the limit is still reached.
    LimitStillReached(StopReason),
    /// This repo does not exist.
    UnknownRepo(String),
    /// Only the system triggers this action (the sync).
    SystemOnly,
    /// There are still reasons against merging.
    MergeBlocked(Vec<MergeBlocker>),
    /// This action comes only from the agent in the container.
    AgentOnly,
    /// Only you may trigger this (not the agent, not the system).
    UserOnly(Actor),
    /// The run has no pushed branch yet that could get a pull request.
    PullRequestNotReady,
    /// The run already has its pull request (number).
    PullRequestExists(u32),
}

impl ActionError {
    /// Explanation for the UI.
    pub fn message(&self) -> String {
        match self {
            ActionError::UnknownItem(id) => format!("Eintrag „{id}“ gibt es nicht"),
            ActionError::UnknownReview(n) => format!("Review {n} gibt es nicht"),
            ActionError::UnknownSuggestion(id) => format!("Vorschlag „{id}“ gibt es nicht"),
            ActionError::UnknownComment(id) => format!("Kommentar „{id}“ gibt es nicht"),
            ActionError::UnknownTicket(key) => format!("Ticket „{key}“ gibt es nicht"),
            ActionError::InvalidInput => "Die Eingabe passt nicht".into(),
            ActionError::NotATicket => "Pull Requests haben keinen Status".into(),
            ActionError::BranchExists => "Es gibt schon einen Branch".into(),
            ActionError::Unchanged => "Es ändert sich nichts".into(),
            ActionError::NothingToDo => "Dafür gibt es gerade nichts zu tun".into(),
            ActionError::Send(SendError::Undecided(n)) => {
                format!("{n} Vorschläge sind noch nicht entschieden")
            }
            ActionError::Send(SendError::AlreadySent) => "Das Review ist schon gesendet".into(),
            ActionError::Send(SendError::UnknownReview) => "Das Review gibt es nicht".into(),
            ActionError::Agent(AgentError::UnknownTicket) => "Dieses Ticket gibt es nicht".into(),
            ActionError::Agent(AgentError::AlreadyRunning) => {
                "Der Agent arbeitet hier schon".into()
            }
            ActionError::Agent(AgentError::UnknownReview) => "Das Review gibt es nicht".into(),
            ActionError::Agent(AgentError::Merged) => {
                "Der Pull Request ist schon gemergt und bleibt, wie er ist".into()
            }
            ActionError::Agent(AgentError::NoComments) => {
                "Wähle mindestens einen offenen Kommentar aus".into()
            }
            ActionError::Agent(AgentError::NotAgentBranch) => {
                "Der Branch dieses Pull Requests passt nicht zum Namensmuster des Repos – dort arbeitet der Agent nicht weiter"
                    .into()
            }
            ActionError::Agent(AgentError::NothingToReview) => {
                "Zu diesem Pull Request gibt es noch keine Änderungen zum Durchsehen. Gleiche zuerst ab."
                    .into()
            }
            ActionError::Agent(AgentError::CheckNotFailed) => {
                "Untersuchen lässt sich nur ein roter Check".into()
            }
            ActionError::Agent(AgentError::ParallelLimit(n)) | ActionError::ParallelLimit(n) => {
                format!(
                    "Es arbeiten schon {n} Agenten – mehr erlaubt die Grenze nicht. Warte, bis einer fertig ist, oder erhöhe die Grenze unter „Grenzen & Regeln“."
                )
            }
            ActionError::PlanViolatesRules(violations) => {
                let reasons: Vec<String> = violations.iter().map(PathViolation::reason).collect();
                format!(
                    "Der Plan lässt sich nicht freigeben: {}. Ändere die Regeln des Repos oder lehne den Plan ab.",
                    reasons.join("; ")
                )
            }
            ActionError::LimitStillReached(reason) => format!(
                "{} – auch mit dieser Erhöhung. Erhöhe um mehr.",
                reason.label()
            ),
            ActionError::UnknownRepo(repo) => format!("Repo „{repo}“ gibt es nicht"),
            ActionError::UnknownCheck(name) => format!("Check „{name}“ gibt es nicht"),
            ActionError::UnknownPerson(name) => format!("{name} gibt es im Team nicht"),
            ActionError::Link(LinkError::UnknownPr(n)) => {
                format!("Pull Request #{n} gibt es nicht")
            }
            ActionError::Link(LinkError::Same) => "Beide sind schon verknüpft".into(),
            ActionError::Link(LinkError::TicketHasPr(n)) => {
                format!("Das Ticket hängt schon an #{n} – löse das zuerst")
            }
            ActionError::Link(LinkError::PrHasTicket(key)) => {
                format!("Der Pull Request gehört schon zu {key}")
            }
            ActionError::Merged(n) => format!("{n} ist schon gemergt und bleibt, wie es ist"),
            ActionError::MergeNotAllowed(actor) => format!(
                "Nur du darfst mergen – {} nicht",
                match actor {
                    Actor::Agent => "der Agent",
                    Actor::System => "das System",
                    Actor::User => "du",
                }
            ),
            ActionError::SystemOnly => {
                "Das macht nur das System – der Abgleich läuft im Server".into()
            }
            ActionError::AgentOnly => "Das meldet nur der Agent aus seinem Container".into(),
            ActionError::UserOnly(actor) => format!(
                "Das darfst nur du auslösen – {} nicht",
                match actor {
                    Actor::Agent => "der Agent",
                    Actor::System => "das System",
                    Actor::User => "du",
                }
            ),
            ActionError::PullRequestNotReady => {
                "Einen Pull Request gibt es erst, wenn der Agent fertig ist und seinen Branch gepusht hat"
                    .into()
            }
            ActionError::PullRequestExists(n) => {
                format!("Zu diesem Lauf gehört schon der Pull Request #{n}")
            }
            ActionError::MergeBlocked(blockers) => {
                let reasons: Vec<&str> = blockers.iter().map(|b| b.reason()).collect();
                format!("Mergen geht noch nicht: {}", reasons.join(" "))
            }
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ActionError {}

/// Error when commissioning: the limit of parallel runs has its own message.
pub(super) fn agent_error(e: AgentError) -> ActionError {
    match e {
        AgentError::ParallelLimit(n) => ActionError::ParallelLimit(n),
        other => ActionError::Agent(other),
    }
}
