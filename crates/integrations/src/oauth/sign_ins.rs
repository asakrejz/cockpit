//! Open sign-ins: the one-time `state` and its PKCE verifier.

use super::*;

/// This long a `state` is valid.
pub const STATE_VALID_MS: u64 = 10 * 60 * 1000;

/// No more open sign-ins at the same time (local, one person).
pub(super) const MAX_OPEN: usize = 16;

pub(super) struct Open {
    service: Service,
    verifier: String,
    ab_ms: u64,
}

/// Why a callback does not match a sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// No `state` or unknown – also a second call of the same callback.
    Unknown,
    Expired,
    /// The `state` belongs to a sign-in at another service.
    WrongService,
}

impl StateError {
    pub fn text(&self) -> &'static str {
        match self {
            StateError::Unknown => {
                "Die Anmeldung ist unbekannt oder wurde schon verwendet – bitte neu starten"
            }
            StateError::Expired => "Die Anmeldung hat zu lange gedauert – bitte neu starten",
            StateError::WrongService => "Die Anmeldung gehört zu einem anderen Dienst",
        }
    }
}

/// Open sign-ins: `state` → PKCE verifier. Single use (deleted when taken) and
/// short-lived. In memory only, never on disk.
#[derive(Default)]
pub struct SignIns {
    pub(super) open: HashMap<String, Open>,
}

impl SignIns {
    /// Starts a sign-in: returns `state` and the PKCE checksum.
    pub fn begin(&mut self, service: Service, now_ms: u64) -> Result<(String, String), Error> {
        self.open
            .retain(|_, o| now_ms.saturating_sub(o.ab_ms) <= STATE_VALID_MS);
        while self.open.len() >= MAX_OPEN {
            // Drop the oldest.
            let oldest = self
                .open
                .iter()
                .min_by_key(|(_, o)| o.ab_ms)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => self.open.remove(&k),
                None => break,
            };
        }
        let state = random(32)?;
        let verifier = random(48)?;
        let challenge = pkce_challenge(&verifier);
        self.open.insert(
            state.clone(),
            Open {
                service,
                verifier,
                ab_ms: now_ms,
            },
        );
        Ok((state, challenge))
    }

    /// Takes the verifier for the `state`. After that it is used up – also on an error.
    pub fn extract(
        &mut self,
        state: &str,
        service: Service,
        now_ms: u64,
    ) -> Result<String, StateError> {
        let o = self.open.remove(state).ok_or(StateError::Unknown)?;
        if now_ms.saturating_sub(o.ab_ms) > STATE_VALID_MS {
            return Err(StateError::Expired);
        }
        if o.service != service {
            return Err(StateError::WrongService);
        }
        Ok(o.verifier)
    }
}
