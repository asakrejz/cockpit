//! Sign-in via OAuth on the page "Einrichten": "Mit … anmelden" and "Trennen".
//! The logic is in the backend; here is only the wiring. The browser
//! sees neither tokens nor secrets: "Anmelden" is a navigation to `/api/oauth/…/start`.

use cockpit_core::api::{ClientInput, ConfigChange};
use cockpit_core::{AuthKind, ServiceStatus};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server::{self, CallError};
use crate::store::use_store;

/// Which service: address in the backend, name, command for the client secret.
#[derive(Clone, Copy)]
pub enum Service {
    GitHub,
    Atlassian,
}

impl Service {
    fn id(self) -> &'static str {
        match self {
            Service::GitHub => "github",
            Service::Atlassian => "atlassian",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Service::GitHub => "GitHub",
            Service::Atlassian => "Atlassian",
        }
    }

    /// Name of the client secret in the .env file.
    fn secret(self) -> &'static str {
        match self {
            Service::GitHub => "github-oauth",
            Service::Atlassian => "atlassian-oauth",
        }
    }

    /// The change that removes the client ID from `config.toml`.
    fn forget_client_id(self) -> ConfigChange {
        let empty = Some(ClientInput::default());
        match self {
            Service::GitHub => ConfigChange {
                github_oauth: empty,
                ..Default::default()
            },
            Service::Atlassian => ConfigChange {
                atlassian: empty,
                ..Default::default()
            },
        }
    }

    fn pick(self, services: &server::Services) -> ServiceStatus {
        match self {
            Service::GitHub => services.github.clone(),
            Service::Atlassian => services.jira.clone(),
        }
    }
}

/// Removes the OAuth app of the service from Cockpit: the client ID from `config.toml` (an empty
/// ID deletes the section including the site choice) and the client secret from the .env file.
/// Both are separate server calls (`PUT /api/config`, `DELETE /api/secrets/{name}`); deleting a
/// secret that is not there is no error. A sign-in must be disconnected first (it needs the
/// client secret to be revoked).
pub async fn forget_client(service: Service) -> Result<(), CallError> {
    server::put_config(&service.forget_client_id()).await?;
    server::delete_key(service.secret()).await?;
    Ok(())
}

/// Text about the state of a connection.
fn describe(status: &ServiceStatus) -> String {
    match (status.connected, status.kind) {
        (true, AuthKind::OAuth) => match &status.site {
            Some(site) => format!("Verbunden per OAuth (Site {site})"),
            None => "Verbunden per OAuth".into(),
        },
        (true, kind) => format!("Verbunden ({})", kind.label()),
        (false, _) => "Getrennt".into(),
    }
}

#[component]
pub fn OAuthCard(
    service: Service,
    /// Called as soon as the server reports the connection as established.
    #[prop(into)]
    on_connected: Callback<()>,
    /// Called after a successful "Trennen".
    #[prop(into)]
    on_disconnected: Callback<()>,
) -> impl IntoView {
    let store = use_store();
    let status = RwSignal::new(None::<ServiceStatus>);

    let load = move || {
        spawn_local(async move {
            if let Ok(services) = server::fetch_services().await {
                let s = service.pick(&services);
                if s.connected {
                    on_connected.run(());
                }
                status.set(Some(s));
            }
        });
    };
    load();

    // "Trennen" can also forget the app (client ID and secret) – off by default.
    let forget = RwSignal::new(false);
    let disconnect = move |_| {
        let forget_too = forget.get_untracked();
        spawn_local(async move {
            match server::post_revoke(service.id()).await {
                Ok(answer) => {
                    let mut hint = answer.hint;
                    if forget_too {
                        match forget_client(service).await {
                            Ok(()) => {
                                forget.set(false);
                                hint = Some(format!(
                                    "Von {} getrennt. Client-ID und Client-Secret sind vergessen.",
                                    service.name()
                                ));
                            }
                            Err(e) => hint = Some(e.message()),
                        }
                    }
                    on_disconnected.run(());
                    store
                        .notify(hint.unwrap_or_else(|| format!("Von {} getrennt", service.name())));
                    if let Ok(services) = server::fetch_services().await {
                        status.set(Some(service.pick(&services)));
                    }
                }
                Err(CallError::Unreachable) => store.notify("Der Server ist nicht erreichbar"),
                Err(e) => store.notify(e.message()),
            }
        });
    };

    view! {
        {move || {
            {
                {
                    let s = status.get().unwrap_or_default();
                    let tone = if s.connected { StatusTone::Green } else { StatusTone::Gray };
                    let oauth = s.kind == AuthKind::OAuth;
                    let possible = s.oauth_possible;
                    let login_label = format!("Mit {} anmelden", service.name());
                    let hint = "Zuerst oben die Client-ID speichern und das Client-Secret ablegen.";
                    view! {
                        <Card>
                            <CardBody css="gap: 12px">
                                <HStack css="justify-content: space-between">
                                    <span class="t-headline">{format!("Anmeldung bei {}", service.name())}</span>
                                    <Status tone=tone>{describe(&s)}</Status>
                                </HStack>
                                <HStack spacing="12px">
                                    {(!oauth)
                                        .then(|| {
                                            view! {
                                                <Button
                                                    variant=ButtonStyle::GlassProminent
                                                    href=format!("/api/oauth/{}/start", service.id())
                                                >
                                                    {login_label}
                                                </Button>
                                            }
                                        })}
                                    {oauth
                                        .then(|| {
                                            view! {
                                                <Button destructive=true on_click=disconnect>
                                                    "Trennen"
                                                </Button>
                                            }
                                        })}
                                </HStack>
                                {oauth
                                    .then(|| {
                                        view! {
                                            <HStack spacing="8px" css="align-items: center">
                                                <Toggle
                                                    checked=forget
                                                    aria_label="Auch Client-ID und Secret vergessen"
                                                />
                                                <span class="t-footnote t-secondary">
                                                    "Auch Client-ID und Secret vergessen"
                                                </span>
                                            </HStack>
                                        }
                                    })}
                                {(!possible && !oauth)
                                    .then(|| view! { <p class="t-footnote t-secondary">{hint}</p> })}
                            </CardBody>
                        </Card>
                    }
                }
            }
        }}
    }
}
