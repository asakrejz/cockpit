//! What each kind of failure of a sign-in service means: the text for the interface, and whether the
//! stored tokens are still of use.

use cockpit_core::{Right, TokenKind};

use super::*;
use crate::oauth::refresh::tokens_unusable;

/// Every kind of failure once.
fn every_error() -> Vec<Error> {
    vec![
        Error::Network("timeout".into()),
        Error::NotSignedIn,
        Error::NoPermission,
        Error::RateLimit { wait_s: Some(30) },
        Error::NotFound,
        Error::TooLarge,
        Error::Rejected(String::new()),
        Error::Rejected("expired".into()),
        Error::Status(500),
        Error::Status(418),
        Error::Response("not json".into()),
        Error::Config("bad address".into()),
        Error::NotPossible("no transition".into()),
        Error::Coded(msg!(IntegrationsNotFound)),
        Error::MissingRight {
            right: Right::ProjectsRead,
            kind: TokenKind::FineGrained,
        },
    ]
}

#[test]
fn a_failed_sign_in_call_is_a_message_that_names_the_service() {
    for d in [Service::GitHub, Service::Atlassian] {
        for e in every_error() {
            let want = match &e {
                Error::NotSignedIn => {
                    msg!(
                        ServerOauthClientRejected,
                        service = d.name(),
                        step = step_name(d)
                    )
                }
                Error::Network(_) => msg!(ServerOauthUnreachable, service = d.name()),
                other => msg!(
                    ServerOauthServiceError,
                    service = d.name(),
                    reason = other.msg()
                ),
            };
            assert_eq!(service_error(d, &e), want, "{e:?}");
        }
    }
}

/// Only a service that no longer accepts the tokens ends the connection; an outage keeps it.
#[test]
fn only_a_refusal_of_the_tokens_ends_the_connection() {
    for e in every_error() {
        let refused = matches!(
            e,
            Error::NotSignedIn | Error::Rejected(_) | Error::NoPermission | Error::Response(_)
        );
        assert_eq!(tokens_unusable(&e), refused, "{e:?}");
    }
}
