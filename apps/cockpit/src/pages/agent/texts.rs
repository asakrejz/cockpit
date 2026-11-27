//! Short names for the visible texts of "finished runs", "Delete branch" and "Clean up".

use feature_ui::t;

pub(super) fn show_closed() -> String {
    t!(AgentCleanupShowClosed)
}

pub(super) fn closed_badge() -> String {
    t!(AgentCleanupClosedBadge)
}

pub(super) fn branch_title() -> String {
    t!(AgentCleanupBranchTitle)
}

pub(super) fn branch_button() -> String {
    t!(AgentCleanupBranchButton)
}

pub(super) fn branch_checking() -> String {
    t!(AgentCleanupBranchChecking)
}

pub(super) fn branch_deleted() -> String {
    t!(AgentCleanupBranchDeleted)
}

pub(super) fn branch_dialog_title() -> String {
    t!(AgentCleanupBranchDialogTitle)
}

pub(super) fn cancel() -> String {
    t!(AgentCleanupCancel)
}

pub(super) fn branch_final() -> String {
    t!(AgentCleanupBranchFinal)
}

pub(super) fn cleanup_button() -> String {
    t!(AgentCleanupCleanupButton)
}

pub(super) fn cleanup_title() -> String {
    t!(AgentCleanupCleanupTitle)
}

pub(super) fn cleanup_loading() -> String {
    t!(AgentCleanupCleanupLoading)
}

pub(super) fn cleanup_intro() -> String {
    t!(AgentCleanupCleanupIntro)
}

pub(super) fn cleanup_nothing() -> String {
    t!(AgentCleanupCleanupNothing)
}

pub(super) fn cleanup_containers() -> String {
    t!(AgentCleanupCleanupContainers)
}

pub(super) fn cleanup_folders() -> String {
    t!(AgentCleanupCleanupFolders)
}

pub(super) fn cleanup_no_engine() -> String {
    t!(AgentCleanupCleanupNoEngine)
}

pub(super) fn cleanup_run() -> String {
    t!(AgentCleanupCleanupRun)
}

pub(super) fn close() -> String {
    t!(AgentCleanupClose)
}

pub(super) fn show_closed_count(n: usize) -> String {
    t!(AgentCleanupShowClosedCount, n = n)
}

pub(super) fn branch_question(branch: &str) -> String {
    t!(AgentCleanupBranchQuestion, branch = branch)
}

pub(super) fn cleanup_done(containers: usize, folders: usize) -> String {
    t!(
        AgentCleanupCleanupDone,
        containers = containers,
        folders = folders
    )
}

pub(super) fn remove_button() -> String {
    t!(AgentCleanupRemoveButton)
}

pub(super) fn remove_title() -> String {
    t!(AgentCleanupRemoveTitle)
}

pub(super) fn remove_question(label: &str) -> String {
    t!(AgentCleanupRemoveQuestion, label = label)
}

pub(super) fn remove_scope() -> String {
    t!(AgentCleanupRemoveScope)
}

pub(super) fn remove_active_hint() -> String {
    t!(AgentCleanupRemoveActiveHint)
}

pub(super) fn remove_all_button(n: usize) -> String {
    t!(AgentCleanupRemoveAllButton, n = n)
}

pub(super) fn remove_all_title() -> String {
    t!(AgentCleanupRemoveAllTitle)
}

pub(super) fn remove_all_question(n: usize) -> String {
    t!(AgentCleanupRemoveAllQuestion, n = n)
}

pub(super) fn remove_all_keep() -> String {
    t!(AgentCleanupRemoveAllKeep)
}

pub(super) fn remove_confirm() -> String {
    t!(AgentCleanupRemoveConfirm)
}
