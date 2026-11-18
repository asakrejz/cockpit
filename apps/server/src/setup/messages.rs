//! The texts of the check results as messages (code and parameters), one per cause.

use cockpit_core::api::{GitHubCause, JiraCause};
use cockpit_i18n::{Msg, msg};

/// The result of the GitHub connection test. Never names the token.
pub(super) fn github_cause_msg(cause: GitHubCause, user: Option<&str>) -> Msg {
    match cause {
        GitHubCause::Ok => match user.filter(|n| !n.is_empty()) {
            Some(n) => msg!(ServerSetupGithubOkUser, user = n),
            None => msg!(ServerSetupGithubOk),
        },
        GitHubCause::NotConnected => msg!(ServerSetupGithubNotConnected),
        GitHubCause::Token => msg!(ServerSetupGithubToken),
        GitHubCause::Permission => msg!(ServerSetupGithubPermission),
        GitHubCause::RateLimit => msg!(ServerSetupGithubRateLimit),
        GitHubCause::Network => msg!(ServerSetupGithubNetwork),
        GitHubCause::Other => msg!(ServerSetupGithubOther),
    }
}

/// The result of the Jira connection test. Never names the token.
pub(super) fn jira_cause_msg(cause: JiraCause, name: Option<&str>) -> Msg {
    match cause {
        JiraCause::Ok => match name.filter(|n| !n.is_empty()) {
            Some(n) => msg!(ServerSetupJiraOkName, name = n),
            None => msg!(ServerSetupJiraOk),
        },
        JiraCause::Details => msg!(ServerSetupJiraDetails),
        JiraCause::Address => msg!(ServerSetupJiraAddress),
        JiraCause::Token => msg!(ServerSetupJiraToken),
        JiraCause::Permission => msg!(ServerSetupJiraPermission),
        JiraCause::Reachable => msg!(ServerSetupJiraReachable),
        JiraCause::Other => msg!(ServerSetupJiraOther),
    }
}

/// Text from outside (a message of the core without a code) as a message.
pub(super) trait RawMsg<T> {
    fn raw_msg(self) -> Result<T, Msg>;
}

impl<T> RawMsg<T> for Result<T, String> {
    fn raw_msg(self) -> Result<T, Msg> {
        self.map_err(Msg::raw)
    }
}
