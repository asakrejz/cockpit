//! Verbindungen zu Jira, GitHub und dem Agenten.
//!
//! [`Connections`] hält nur Haken – **nie Schlüssel**. Die `check_*`-Funktionen prüfen
//! das Format der Eingaben. Ob ein Schlüssel wirklich gilt, kann nur der Dienst sagen.

/// Welche Verbindungen stehen und was der Agent selbstständig darf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connections {
    pub jira: bool,
    pub github: bool,
    pub agent: bool,
    pub may_branch: bool,
    pub may_open_pr: bool,
    pub may_run_tests: bool,
}

impl Default for Connections {
    fn default() -> Self {
        Self {
            jira: false,
            github: false,
            agent: false,
            may_branch: true,
            may_open_pr: true,
            may_run_tests: true,
        }
    }
}

impl Connections {
    pub fn all_connected(&self) -> bool {
        self.jira && self.github && self.agent
    }

    /// Warum der Agent kein Ticket umsetzen darf – oder `None`, wenn er darf.
    pub fn agent_blocked(&self) -> Option<&'static str> {
        if !self.agent {
            Some("Der Agent ist nicht verbunden.")
        } else if !self.may_branch {
            Some("Der Agent darf keine Branches erstellen.")
        } else if !self.may_open_pr {
            Some("Der Agent darf keine Pull Requests erstellen.")
        } else {
            None
        }
    }

    fn flags(&self) -> [bool; 6] {
        [
            self.jira,
            self.github,
            self.agent,
            self.may_branch,
            self.may_open_pr,
            self.may_run_tests,
        ]
    }

    /// Sechs Zeichen aus `0` und `1`, zum Speichern.
    pub fn encode(&self) -> String {
        self.flags()
            .iter()
            .map(|&on| if on { '1' } else { '0' })
            .collect()
    }

    /// Gegenstück zu [`encode`](Self::encode). Unlesbares ergibt den Standard.
    pub fn decode(text: &str) -> Self {
        let bits: Vec<bool> = text.chars().map(|c| c == '1').collect();
        match bits[..] {
            [jira, github, agent, may_branch, may_open_pr, may_run_tests]
                if text.chars().all(|c| c == '0' || c == '1') =>
            {
                Self {
                    jira,
                    github,
                    agent,
                    may_branch,
                    may_open_pr,
                    may_run_tests,
                }
            }
            _ => Self::default(),
        }
    }
}

fn is_token(text: &str, min: usize) -> bool {
    text.len() >= min && !text.chars().any(char::is_whitespace)
}

/// Prüft die Eingaben für Jira. Der Fehler ist eine Meldung für die Oberfläche.
pub fn check_jira(url: &str, email: &str, token: &str) -> Result<(), String> {
    let (url, email, token) = (url.trim(), email.trim(), token.trim());
    let host = url
        .strip_prefix("https://")
        .map(|h| h.trim_end_matches('/'));
    if url.is_empty() {
        return Err("Bitte die Jira-Adresse eingeben.".into());
    }
    if !host.is_some_and(|h| h.contains('.') && is_token(h, 4) && !h.contains('/')) {
        return Err("Die Adresse muss so aussehen: https://firma.atlassian.net".into());
    }
    let mail_ok = email
        .split_once('@')
        .is_some_and(|(name, domain)| !name.is_empty() && domain.contains('.'));
    if !mail_ok || !is_token(email, 5) {
        return Err("Bitte eine gültige E-Mail-Adresse eingeben.".into());
    }
    if !is_token(token, 12) {
        return Err("Das API-Token fehlt oder ist zu kurz.".into());
    }
    Ok(())
}

/// Prüft Token und Repository-Liste für GitHub. Gibt die Repositories zurück.
pub fn check_github(token: &str, repos: &str) -> Result<Vec<String>, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Bitte das Personal Access Token eingeben.".into());
    }
    if !(token.starts_with("github_pat_") || token.starts_with("ghp_")) || !is_token(token, 20) {
        return Err("Das Token beginnt mit github_pat_ oder ghp_ und ist länger.".into());
    }
    let list: Vec<String> = repos
        .split([',', ' ', '\n'])
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .collect();
    if list.is_empty() {
        return Err("Bitte mindestens ein Repository angeben.".into());
    }
    let part_ok = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    for repo in &list {
        let ok = repo
            .split_once('/')
            .is_some_and(|(owner, name)| part_ok(owner) && part_ok(name));
        if !ok {
            return Err(format!(
                "„{repo}“ hat nicht die Form organisation/repository."
            ));
        }
    }
    Ok(list)
}

/// Prüft den API-Key des Modell-Anbieters.
pub fn check_agent(key: &str) -> Result<(), String> {
    if is_token(key.trim(), 16) {
        Ok(())
    } else {
        Err("Der API-Key fehlt oder ist zu kurz.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let c = Connections {
            jira: true,
            github: false,
            agent: true,
            may_branch: false,
            may_open_pr: true,
            may_run_tests: false,
        };
        assert_eq!(c.encode(), "101010");
        assert_eq!(Connections::decode("101010"), c);
        assert_eq!(
            Connections::decode(&Connections::default().encode()),
            Connections::default()
        );
    }

    #[test]
    fn unreadable_input_gives_the_default() {
        for bad in ["", "1", "1111111", "11x111", "äöüäöü"] {
            assert_eq!(Connections::decode(bad), Connections::default(), "{bad}");
        }
    }

    #[test]
    fn agent_needs_connection_and_permissions() {
        let mut c = Connections::default();
        assert!(c.agent_blocked().is_some());
        c.agent = true;
        assert_eq!(c.agent_blocked(), None);
        c.may_open_pr = false;
        assert!(c.agent_blocked().unwrap().contains("Pull Requests"));
        c.may_branch = false;
        assert!(c.agent_blocked().unwrap().contains("Branches"));
        c.may_branch = true;
        c.may_open_pr = true;
        c.may_run_tests = false;
        assert_eq!(c.agent_blocked(), None, "Tests sind optional");
    }

    #[test]
    fn jira() {
        let token = "abcdefghijkl";
        assert!(check_jira("https://firma.atlassian.net", "a@b.de", token).is_ok());
        assert!(check_jira(" https://firma.atlassian.net/ ", " a@b.de ", token).is_ok());
        assert!(
            check_jira("", "a@b.de", token)
                .unwrap_err()
                .contains("Jira-Adresse")
        );
        for url in [
            "http://firma.atlassian.net",
            "https://localhost",
            "firma.atlassian.net",
            "https://a.b/pfad",
        ] {
            assert!(
                check_jira(url, "a@b.de", token)
                    .unwrap_err()
                    .contains("https://"),
                "{url}"
            );
        }
        for mail in ["", "kein-at", "@b.de", "a@b", "a b@c.de"] {
            assert!(
                check_jira("https://x.de", mail, token)
                    .unwrap_err()
                    .contains("E-Mail"),
                "{mail}"
            );
        }
        assert!(
            check_jira("https://x.de", "a@b.de", "kurz")
                .unwrap_err()
                .contains("API-Token")
        );
    }

    #[test]
    fn github() {
        let token = "github_pat_ABCDEFGHIJKLMNOP";
        assert_eq!(
            check_github(token, "acme/app, acme/docs.site"),
            Ok(vec!["acme/app".into(), "acme/docs.site".into()])
        );
        assert!(check_github("ghp_ABCDEFGHIJKLMNOPQRST", "a/b").is_ok());
        assert!(check_github("", "a/b").is_err());
        assert!(check_github("irgendwas-langes-ohne-praefix", "a/b").is_err());
        assert!(check_github("ghp_kurz", "a/b").is_err());
        assert!(
            check_github(token, "  ")
                .unwrap_err()
                .contains("mindestens ein")
        );
        for repo in ["nur-name", "a/b/c", "/b", "a/", "a/b?c"] {
            assert!(
                check_github(token, repo).unwrap_err().contains(repo),
                "{repo}"
            );
        }
    }

    #[test]
    fn agent() {
        assert!(check_agent("sk-0123456789abcdef").is_ok());
        assert!(check_agent("").is_err());
        assert!(check_agent("sk-kurz").is_err());
        assert!(check_agent("sk-mit leerzeichen 123456").is_err());
    }
}
