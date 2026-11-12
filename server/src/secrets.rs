//! Ablage der Schlüssel. Sie liegen im Schlüsselbund des Systems, nie in der Datenbank,
//! nie in der Konfigurationsdatei. `Secret` zeigt sich in `Debug` nur als `Secret(***)`.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use cockpit_integrations::Secret;

/// Name des Eintrags im Schlüsselbund.
const DIENST: &str = "cockpit";

/// Namen der Schlüssel, die das Backend kennt.
pub const GITHUB: &str = "github";
pub const JIRA: &str = "jira";
/// Client-Secrets der OAuth-Apps (`token setzen github-oauth|atlassian-oauth`).
pub const GITHUB_OAUTH: &str = "github-oauth";
pub const ATLASSIAN_OAUTH: &str = "atlassian-oauth";
/// Key für den LiteLLM-Proxy (`token setzen litellm`); geht nur beim Start an den Agent-Container.
pub const LITELLM: &str = "litellm";
/// Access- und Refresh-Token nach der Anmeldung (ein Eintrag je Dienst, vom Backend gesetzt).
pub const GITHUB_OAUTH_TOKENS: &str = "github-oauth-token";
pub const ATLASSIAN_OAUTH_TOKENS: &str = "atlassian-oauth-token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFehler(pub String);

impl std::fmt::Display for SecretFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Schlüsselablage: {}", self.0)
    }
}

impl std::error::Error for SecretFehler {}

pub trait SecretStore: Send + Sync {
    /// `Ok(None)`, wenn es keinen Schlüssel dieses Namens gibt.
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretFehler>;
    fn set(&self, name: &str, wert: &Secret) -> Result<(), SecretFehler>;
    /// Löschen eines nicht vorhandenen Schlüssels ist kein Fehler.
    fn delete(&self, name: &str) -> Result<(), SecretFehler>;

    fn has(&self, name: &str) -> Result<bool, SecretFehler> {
        Ok(self.get(name)?.is_some())
    }
}

/// Schlüsselbund des Systems (macOS Keychain, Windows Credential Manager, Secret Service
/// unter Linux).
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    fn eintrag(name: &str) -> Result<keyring::Entry, SecretFehler> {
        keyring::Entry::new(DIENST, name).map_err(fehler)
    }
}

/// Die Fehlermeldung des Schlüsselbunds enthält nie den Wert.
fn fehler(e: keyring::Error) -> SecretFehler {
    SecretFehler(e.to_string())
}

impl SecretStore for KeyringStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretFehler> {
        match Self::eintrag(name)?.get_password() {
            Ok(w) => Ok(Some(Secret::new(w))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(fehler(e)),
        }
    }

    fn set(&self, name: &str, wert: &Secret) -> Result<(), SecretFehler> {
        Self::eintrag(name)?
            .set_password(wert.expose())
            .map_err(fehler)
    }

    fn delete(&self, name: &str) -> Result<(), SecretFehler> {
        match Self::eintrag(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(fehler(e)),
        }
    }
}

/// Nur im Speicher – für Tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, Secret>>);

#[cfg(test)]
impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Nur die Namen, nie die Werte.
        let namen: Vec<String> = self
            .0
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        f.debug_struct("MemoryStore")
            .field("namen", &namen)
            .finish()
    }
}

#[cfg(test)]
impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretFehler> {
        Ok(self.0.lock().expect("Sperre vergiftet").get(name).cloned())
    }

    fn set(&self, name: &str, wert: &Secret) -> Result<(), SecretFehler> {
        self.0
            .lock()
            .expect("Sperre vergiftet")
            .insert(name.into(), wert.clone());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretFehler> {
        self.0.lock().expect("Sperre vergiftet").remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setzen_lesen_loeschen() {
        let s = MemoryStore::new();
        assert_eq!(s.get(GITHUB).unwrap(), None);
        assert!(!s.has(GITHUB).unwrap());
        s.set(GITHUB, &Secret::new("ghp_a")).unwrap();
        s.set(JIRA, &Secret::new("j")).unwrap();
        assert_eq!(s.get(GITHUB).unwrap().unwrap().expose(), "ghp_a");
        assert!(s.has(GITHUB).unwrap());
        s.set(GITHUB, &Secret::new("ghp_b")).unwrap();
        assert_eq!(s.get(GITHUB).unwrap().unwrap().expose(), "ghp_b");
        s.delete(GITHUB).unwrap();
        s.delete(GITHUB).unwrap(); // zweites Löschen ist kein Fehler
        assert_eq!(s.get(GITHUB).unwrap(), None);
        assert!(s.has(JIRA).unwrap());
    }

    #[test]
    fn debug_zeigt_keine_werte() {
        let s = MemoryStore::new();
        s.set(GITHUB, &Secret::new("ghp_geheim")).unwrap();
        let ausgabe = format!("{s:?} {:?}", s.get(GITHUB).unwrap());
        assert!(!ausgabe.contains("ghp_geheim"), "{ausgabe}");
        assert!(ausgabe.contains("github"));
    }
}
