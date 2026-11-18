//! The visible texts of „erledigte Läufe“, „Branch löschen“ and „Aufräumen“, collected in one
//! place so that they can later move into the language files.

pub(super) const SHOW_CLOSED: &str = "Erledigte anzeigen";
pub(super) fn show_closed_count(n: usize) -> String {
    format!("{SHOW_CLOSED} ({n})")
}
pub(super) const CLOSED_BADGE: &str = "erledigt";

pub(super) const BRANCH_TITLE: &str = "Branch";
pub(super) const BRANCH_BUTTON: &str = "Branch löschen";
pub(super) const BRANCH_CHECKING: &str = "Das Cockpit prüft bei GitHub, ob der Branch weg darf …";
pub(super) const BRANCH_DELETED: &str = "Der Branch ist auf GitHub gelöscht.";
pub(super) const BRANCH_DIALOG_TITLE: &str = "Branch löschen?";
pub(super) const CANCEL: &str = "Abbrechen";
pub(super) fn branch_question(branch: &str) -> String {
    format!(
        "Soll der Branch „{branch}“ auf GitHub gelöscht werden? Nur dieser eine Branch geht weg. Der Pull Request und seine Änderungen bleiben auf GitHub sichtbar."
    )
}
pub(super) const BRANCH_FINAL: &str = "Das lässt sich im Cockpit nicht rückgängig machen.";

pub(super) const CLEANUP_BUTTON: &str = "Aufräumen";
pub(super) const CLEANUP_TITLE: &str = "Aufräumen";
pub(super) const CLEANUP_LOADING: &str = "Das Cockpit sucht nach Resten …";
pub(super) const CLEANUP_INTRO: &str = "Das wird entfernt. Das Image des Agenten und fremde Container bleiben, ebenso Läufe, die gerade arbeiten oder warten.";
pub(super) const CLEANUP_NOTHING: &str = "Nichts zu tun. Es gibt keine Reste.";
pub(super) const CLEANUP_CONTAINERS: &str = "Container ohne laufenden Lauf";
pub(super) const CLEANUP_FOLDERS: &str = "Ordner ohne laufenden Lauf";
pub(super) const CLEANUP_NO_ENGINE: &str =
    "Das Container-Programm ist nicht erreichbar. Container werden deshalb nicht angezeigt.";
pub(super) const CLEANUP_RUN: &str = "Jetzt entfernen";
pub(super) const CLOSE: &str = "Schließen";
pub(super) fn cleanup_done(containers: usize, folders: usize) -> String {
    format!("Entfernt: {containers} Container, {folders} Ordner.")
}
