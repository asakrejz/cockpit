use super::*;

/// Mergen: Methode wählen, Button – und warum er gerade nicht geht.
pub(super) fn merge_section(store: Store, r: &Review, panel: Panel) -> AnyView {
    let number = r.number;
    if let Some(method) = r.merged {
        return view! {
            <div class="ui-vstack" style="--spacing: 10px">
                <h3 class="t-headline">"Mergen"</h3>
                <Status tone=StatusTone::Indigo>{format!("Gemergt ({})", method.label())}</Status>
            </div>
        }
        .into_any();
    }
    // Der Workspace kennt auch den Entwurfs-Status aus der Inbox.
    let blockers = store.ws.with_untracked(|w| w.merge_blockers(number));
    let can_merge = blockers.is_empty();
    let reasons = blockers
        .iter()
        .map(|b| view! { <li class="t-footnote t-secondary">{b.reason()}</li> })
        .collect_view();
    let merge = move |_| {
        let Some(method) = MergeMethod::from_id(panel.merge_method.get_untracked()) else {
            return;
        };
        let _ = store.dispatch(Action::MergePr {
            review: number,
            method,
        });
    };
    view! {
        <div class="ui-vstack" style="--spacing: 10px">
            <h3 class="t-headline">"Mergen"</h3>
            <SegmentedPicker
                options=MergeMethod::OPTIONS
                value=panel.merge_method
                on_change=move |id: &'static str| panel.merge_method.set(id)
                aria_label="Merge-Methode"
                stretch=true
            />
            <Button variant=ButtonStyle::BorderedProminent size=ControlSize::Large disabled=!can_merge on_click=merge>
                "Mergen"
            </Button>
            {(!can_merge)
                .then(|| {
                    view! {
                        <ul class="ui-vstack" style="--spacing: 4px; margin: 0; padding-inline-start: 18px">
                            {reasons}
                        </ul>
                    }
                })}
        </div>
    }
    .into_any()
}
