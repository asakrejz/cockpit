//! Sign-in via OAuth on the page "Einrichten": "Mit … anmelden" and "Trennen".
//! The logic is in the backend; here is only the wiring. The browser
//! sees neither tokens nor secrets: "Anmelden" is a navigation to `/api/oauth/…/start`.

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

    fn pick(self, services: &server::Services) -> ServiceStatus {
        match self {
            Service::GitHub => services.github.clone(),
            Service::Atlassian => services.jira.clone(),
        }
    }
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

    let disconnect = move |_| {
        spawn_local(async move {
            match server::post_revoke(service.id()).await {
                Ok(answer) => {
                    on_disconnected.run(());
                    store.notify(
                        answer
                            .hint
                            .unwrap_or_else(|| format!("Von {} getrennt", service.name())),
                    );
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
                    let hint = format!(
                        "Zuerst oben die Client-ID speichern und das Client-Secret ablegen (Terminal-Weg: „cockpit-server token set {}-oauth“).",
                        service.id(),
                    );
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
