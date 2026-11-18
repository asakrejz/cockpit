use super::*;

/// Why merging is not possible yet.
fn blocker_reason(b: cockpit_core::MergeBlocker) -> String {
    use cockpit_core::MergeBlocker as B;
    match b {
        B::AlreadyMerged => t!(ReviewMergeAlreadyMerged),
        B::Draft => t!(ReviewMergeDraft),
        B::ChecksFailed => t!(ReviewMergeChecksFailed),
        B::ChecksRunning => t!(ReviewMergeChecksRunning),
        B::ReviewNotSent => t!(ReviewMergeReviewNotSent),
        B::ChangesRequested => t!(ReviewMergeChangesRequested),
    }
}

/// Merge: choose method, button – and why it does not work right now.
pub(super) fn merge_section(store: Store, r: &Review, panel: Panel) -> AnyView {
    let number = StoredValue::new(r.key());
    if let Some(method) = r.merged {
        return view! {
            <div class="ui-vstack" style="--spacing: 10px">
                <h3 class="t-headline">{t!(ReviewMergeTitle)}</h3>
                <Status tone=StatusTone::Indigo>{t!(ReviewMerged, method = method.label())}</Status>
            </div>
        }
        .into_any();
    }
    // The workspace also knows the draft status from the inbox.
    let blockers = store
        .ws
        .with_untracked(|w| w.merge_blockers(number.get_value()));
    let can_merge = blockers.is_empty();
    let reasons = blockers
        .iter()
        .map(|b| view! { <li class="t-footnote t-secondary">{blocker_reason(*b)}</li> })
        .collect_view();
    let merge = move |_| {
        let Some(method) = MergeMethod::from_id(panel.merge_method.get_untracked()) else {
            return;
        };
        let _ = store.dispatch(Action::MergePr {
            review: number.get_value(),
            method,
        });
    };
    view! {
        <div class="ui-vstack" style="--spacing: 10px">
            <h3 class="t-headline">{t!(ReviewMergeTitle)}</h3>
            {segmented(
                MergeMethod::OPTIONS.iter().map(|(id, label)| (*id, label.to_string())).collect(),
                move || panel.merge_method.get(),
                move |id| panel.merge_method.set(id),
                t!(ReviewMergeMethod),
                true,
            )}
            <Button variant=ButtonStyle::BorderedProminent size=ControlSize::Large disabled=!can_merge on_click=merge>
                {t!(ReviewMergeButton)}
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
