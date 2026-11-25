//! Step 4: model – tool of the agent, endpoint (directly at Anthropic or OpenAI, own proxy such as
//! LiteLLM or another compatible endpoint), key, model name as a drop-down from the list of the
//! endpoint and the connection tests. The rules are in `cockpit_core::setup`.

use cockpit_core::api::{ConfigChange, EndpointInput, Provider};
use cockpit_core::setup::{
    EndpointKind, Tone, check_plain_text, model_hint, model_name_read, model_selection,
};
use feature_ui::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::fields::{Context, Feedback, Intro, KeyField, Message, Section, config_save};
use crate::server::{self, CallError};
use crate::view::setup_tone;

/// The text of an endpoint kind in the drop-down (the direct way names the vendor of the tool).
fn kind_label(kind: EndpointKind, tool: Provider) -> String {
    match kind {
        EndpointKind::Direct => t!(SetupModelKindDirect, vendor = tool.vendor()),
        EndpointKind::Proxy => t!(SetupModelKindProxy),
        EndpointKind::Compatible => t!(SetupModelKindCompatible),
    }
}

/// Explanation under the endpoint selection.
fn kind_explanation(kind: EndpointKind, tool: Provider) -> String {
    match kind {
        EndpointKind::Direct => t!(SetupModelExplainDirect, vendor = tool.vendor()),
        EndpointKind::Proxy => t!(SetupModelExplainProxy),
        EndpointKind::Compatible => t!(SetupModelExplainCompatible, vendor = tool.vendor()),
    }
}

/// Example address in the field for the endpoint address.
fn kind_placeholder(kind: EndpointKind) -> &'static str {
    match kind {
        EndpointKind::Direct => "",
        EndpointKind::Proxy => "https://litellm.example",
        EndpointKind::Compatible => "https://endpoint.example",
    }
}

/// Label of the key field.
fn kind_key_label(kind: EndpointKind, tool: Provider) -> String {
    match kind {
        EndpointKind::Direct if tool.openai_compatible() => t!(SetupModelKeyOpenai),
        EndpointKind::Direct => t!(SetupModelKeyAnthropic),
        EndpointKind::Proxy => t!(SetupModelKeyProxy),
        EndpointKind::Compatible => t!(SetupModelKeyEndpoint),
    }
}

#[component]
pub fn ModelStep(ctx: Context) -> impl IntoView {
    let provider = RwSignal::new(Provider::default());
    let art = RwSignal::new(EndpointKind::Direct);
    let url = RwSignal::new(String::new());
    let model = RwSignal::new(String::new());
    let from_hand = RwSignal::new(String::new());
    let list = RwSignal::new(Vec::<String>::new());
    let list_loads = RwSignal::new(false);
    let list_hint = RwSignal::new(None::<String>);
    // As soon as the person changes something themselves, the state from the server no longer overwrites their input.
    let touched = RwSignal::new(false);
    Effect::new(move |_| {
        let e = ctx.setup.get();
        if touched.get_untracked() {
            return;
        }
        let address = e.endpoint_url.unwrap_or_default();
        art.set(EndpointKind::from_url(&address));
        url.set(address);
        model.set(e.endpoint_model.unwrap_or_default());
        provider.set(e.provider);
    });
    let back: Feedback = RwSignal::new(None);
    let test_running = RwSignal::new(false);
    let test_error = RwSignal::new(None::<String>);

    // The model that is saved: typed by hand goes before the selection.
    let chosen_model = move || -> Result<String, String> {
        let hand = from_hand.get_untracked();
        if hand.trim().is_empty() {
            Ok(model.get_untracked().trim().to_string())
        } else {
            model_name_read(&hand).map_err(|m| m.render(lang_untracked()))
        }
    };
    let input = move |model: Option<String>| EndpointInput {
        provider: Some(provider.get_untracked()),
        // Directly at Anthropic: no address.
        url: Some(if art.get_untracked().needs_address() {
            url.get_untracked()
        } else {
            String::new()
        }),
        model,
    };

    let save = move |_| match chosen_model() {
        Ok(m) => {
            let hand = !from_hand.get_untracked().trim().is_empty();
            config_save(
                ctx,
                ConfigChange {
                    endpoint: Some(input(Some(m.clone()))),
                    ..Default::default()
                },
                back,
                t!(SetupModelSaved),
                move || {
                    if hand {
                        model.set(m.clone());
                        from_hand.set(String::new());
                    }
                },
            )
        }
        Err(m) => back.set(Some((false, m))),
    };
    // "Modelle laden": first save tool and address (the server asks the saved
    // endpoint with the key from the .env file), then fetch the list.
    let load_models = move |_| {
        list_loads.set(true);
        list_hint.set(None);
        let change = ConfigChange {
            endpoint: Some(input(None)),
            ..Default::default()
        };
        spawn_local(async move {
            let error = |e: CallError| match e {
                CallError::Unreachable => t!(SetupModelUnreachable),
                e => e.message(),
            };
            if let Err(e) = server::put_config(&change).await {
                list_hint.set(Some(error(e)));
                list_loads.set(false);
                return;
            }
            ctx.reload();
            match server::fetch_models().await {
                Ok(l) if l.loaded => {
                    list_hint.set(Some(t!(SetupModelLoadedCount, n = l.models.len())));
                    list.set(l.models);
                }
                Ok(l) => list_hint.set(Some(
                    l.hint
                        .map(|h| server::shown(&h, l.msg.as_ref()))
                        .unwrap_or_else(|| t!(SetupModelNoModels)),
                )),
                Err(e) => list_hint.set(Some(error(e))),
            }
            list_loads.set(false);
        });
    };
    let test = move |_| {
        test_running.set(true);
        test_error.set(None);
        spawn_local(async move {
            match server::fetch_endpoint_check().await {
                Ok(p) => ctx.check.set(Some(p)),
                Err(CallError::Unreachable) => test_error.set(Some(t!(SetupModelUnreachable))),
                Err(e) => test_error.set(Some(e.message())),
            }
            test_running.set(false);
        });
    };
    let c_running = RwSignal::new(false);
    let c_error = RwSignal::new(None::<String>);
    let from_container = move |_| {
        c_running.set(true);
        c_error.set(None);
        spawn_local(async move {
            match server::fetch_container_check().await {
                Ok(p) => ctx.container_check.set(Some(p)),
                Err(CallError::Unreachable) => c_error.set(Some(t!(SetupModelUnreachable))),
                Err(e) => c_error.set(Some(e.message())),
            }
            c_running.set(false);
        });
    };
    let ready = move || {
        ctx.store.health.with(|h| {
            h.as_ref()
                .is_some_and(|h| h.endpoint.configured && h.endpoint.key)
        })
    };

    let key_set = move || ctx.setup.with(|e| e.has_key("agent-key"));
    let provider_options = Signal::derive(move || {
        Provider::ALL
            .into_iter()
            .map(|a| (a.id().to_string(), a.label().to_string()))
            .collect::<Vec<_>>()
    });
    let kind_options = Signal::derive(move || {
        EndpointKind::ALL
            .into_iter()
            .map(|a| (a.id().to_string(), kind_label(a, provider.get())))
            .collect::<Vec<_>>()
    });
    let model_options = Signal::derive(move || {
        let l = list.get();
        let mut o: Vec<(String, String)> = Vec::new();
        let chosen = model.get();
        if chosen.trim().is_empty() {
            o.push((String::new(), t!(SetupModelChoose)));
        }
        o.extend(model_selection(&l, &chosen).into_iter().map(|m| {
            // "Not in the list" only applies if there is a list at all.
            let text = if m.unknown && !l.is_empty() {
                t!(SetupModelNotInList, id = m.id)
            } else {
                m.id.clone()
            };
            (m.id, text)
        }));
        o
    });

    view! {
        <VStack class="ui-gap-12">
            <Intro title=t!(SetupModelTitle)>
                <p>{t!(SetupModelIntro)}</p>
                <p>
                    <strong>{t!(SetupModelCannotCreate)}</strong>
                    " "
                    {t!(SetupModelCannotCreateText)}
                </p>
            </Intro>
            <Section title=t!(SetupModelS1)>
                <Select
                    label=t!(SetupModelToolLabel)
                    options=provider_options
                    value=Signal::derive(move || provider.get().id().to_string())
                    on_change=Callback::new(move |v: String| {
                        if let Some(a) = Provider::from_id(&v) {
                            touched.set(true);
                            provider.set(a);
                        }
                    })
                />
                <p class="t-footnote t-secondary">
                    {move || {
                        if provider.get().openai_compatible() {
                            t!(SetupModelToolCodex)
                        } else {
                            t!(SetupModelToolOther)
                        }
                    }}
                </p>
            </Section>
            <Section title=t!(SetupModelS2)>
                <Select
                    label=t!(SetupModelKindLabel)
                    options=kind_options
                    value=Signal::derive(move || art.get().id().to_string())
                    on_change=Callback::new(move |v: String| {
                        if let Some(a) = EndpointKind::from_id(&v) {
                            touched.set(true);
                            art.set(a);
                        }
                    })
                />
                <p class="t-footnote t-secondary">{move || kind_explanation(art.get(), provider.get())}</p>
                {move || {
                    art.get()
                        .needs_address()
                        .then(|| {
                            view! {
                                <TextField
                                    label=t!(SetupModelAddressLabel)
                                    value=url
                                    placeholder=kind_placeholder(art.get())
                                    hint=t!(SetupModelAddressHint)
                                    autocomplete="off"
                                />
                            }
                        })
                }}
            </Section>
            <Section title=t!(SetupModelS3)>
                {move || {
                    view! {
                        <KeyField
                            ctx=ctx
                            name="agent-key"
                            label=kind_key_label(art.get(), provider.get())
                            hint=t!(SetupModelKeyHint)
                        />
                    }
                }}
            </Section>
            <Section title=t!(SetupModelS4)>
                <p class="t-subheadline t-secondary">
                    {t!(SetupModelListText)}
                </p>
                <HStack class="ui-gap-12">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || {
                            list_loads.get() || !key_set()
                                || (art.get().needs_address() && url.with(|u| u.trim().is_empty()))
                        })
                        on_click=load_models
                    >
                        {move || if list_loads.get() { t!(SetupModelLoading) } else { t!(SetupModelLoad) }}
                    </Button>
                </HStack>
                {move || {
                    (!key_set())
                        .then(|| {
                            view! {
                                <p class="t-footnote t-secondary">
                                    {t!(SetupModelKeyFirst)}
                                </p>
                            }
                        })
                }}
                {move || {
                    list_hint
                        .get()
                        .map(|m| view! { <p class="ui-hint" role="status">{m}</p> })
                }}
                <Select
                    label=t!(SetupModelLabel)
                    options=model_options
                    value=Signal::derive(move || model.get())
                    on_change=Callback::new(move |v: String| {
                        touched.set(true);
                        model.set(v);
                    })
                    disabled=Signal::derive(move || {
                        list.with(Vec::is_empty) && model.with(|m| m.trim().is_empty())
                    })
                />
                {move || {
                    model_hint(&list.get(), &model.get())
                        .map(|m| view! { <p class="ui-hint" role="status">{m.render(lang())}</p> })
                }}
                <TextField
                    label=t!(SetupModelHandLabel)
                    value=from_hand
                    placeholder=t!(SetupModelHandPlaceholder)
                    hint=t!(SetupModelHandHint)
                    autocomplete="off"
                />
                <HStack class="ui-gap-12">
                    <Button variant=ButtonStyle::GlassProminent on_click=save>
                        {t!(SetupModelSave)}
                    </Button>
                </HStack>
                <Message feedback=back />
            </Section>
            <Section title=t!(SetupModelS5)>
                <p class="t-subheadline t-secondary">
                    {t!(SetupModelTestText)}
                </p>
                <HStack class="ui-gap-12">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || test_running.get() || !ready())
                        on_click=test
                    >
                        {move || if test_running.get() { t!(SetupModelTesting) } else { t!(SetupModelTest) }}
                    </Button>
                </HStack>
                {move || {
                    (!ready())
                        .then(|| {
                            view! {
                                <p class="t-footnote t-secondary">
                                    {t!(SetupModelTestNeeds)}
                                </p>
                            }
                        })
                }}
                {move || {
                    test_error
                        .get()
                        .map(|m| view! { <p class="ui-hint" role="alert">{m}</p> })
                }}
                {move || {
                    ctx.check
                        .get()
                        .map(|p| {
                            let (ton, text) = check_plain_text(&p);
                            let text = text.render(lang());
                            let tone = setup_tone(ton);
                            view! {
                                <HStack class="ui-gap-8">
                                    <Status tone=tone>{if ton == Tone::Good { t!(SetupModelSucceeded) } else { t!(SetupModelFailed) }}</Status>
                                    <span class="t-subheadline">{text}</span>
                                </HStack>
                            }
                        })
                }}
            </Section>
            <Section title=t!(SetupModelS6)>
                <p class="t-subheadline t-secondary">
                    {t!(SetupModelContainerText)}
                </p>
                <HStack class="ui-gap-12">
                    <Button
                        variant=ButtonStyle::GlassProminent
                        disabled=Signal::derive(move || c_running.get() || !ready())
                        on_click=from_container
                    >
                        {move || if c_running.get() { t!(SetupModelContainerTesting) } else { t!(SetupModelContainerTest) }}
                    </Button>
                </HStack>
                {move || {
                    c_error
                        .get()
                        .map(|m| view! { <p class="ui-hint" role="alert">{m}</p> })
                }}
                {move || {
                    ctx.container_check
                        .get()
                        .map(|p| {
                            let gut = p.checked && p.container_reached && p.key_ok;
                            let tone = if gut {
                                StatusTone::Green
                            } else if p.checked {
                                StatusTone::Red
                            } else {
                                StatusTone::Gray
                            };
                            view! {
                                <HStack class="ui-gap-8">
                                    <Status tone=tone>
                                        {if gut { t!(SetupModelSucceeded) } else if p.checked { t!(SetupModelFailed) } else { t!(SetupModelNotChecked) }}
                                    </Status>
                                    <span class="t-subheadline">{p.hint.unwrap_or_default()}</span>
                                </HStack>
                            }
                        })
                }}
            </Section>
        </VStack>
    }
}
