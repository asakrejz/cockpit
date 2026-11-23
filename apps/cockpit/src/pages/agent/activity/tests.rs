use super::*;
use cockpit_core::LineKind;

fn run_with(files: &[&str]) -> AgentRun {
    let mut run = AgentRun::new("BAU-142", "Toggle: Fokus-Ring fehlt im Dark Mode");
    run.files = files.iter().map(|f| (*f).to_string()).collect();
    run
}

fn names(list: &[ChangedFile]) -> Vec<&str> {
    list.iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn a_run_lists_the_files_the_backend_reported() {
    let run = run_with(&["src/toggle.rs", "src/tokens.css"]);
    let list = changed_file_list(&run, None);
    assert_eq!(names(&list), ["src/toggle.rs", "src/tokens.css"]);
    assert!(list.iter().all(|f| f.stat.is_none()));
}

#[test]
fn a_run_without_files_lists_none_for_any_ticket() {
    for key in ["BAU-142", "BAU-138", "ABC-1"] {
        let run = AgentRun::new(key, "Titel");
        assert!(changed_file_list(&run, None).is_empty(), "{key}");
    }
}

#[test]
fn the_diff_of_the_pull_request_wins_and_brings_the_line_counts() {
    let run = run_with(&["src/toggle.rs"]);
    let diffs = [FileDiff::new(
        "src/toggle.rs",
        &[(3, LineKind::Add, "a"), (4, LineKind::Del, "b")],
    )];
    let list = changed_file_list(&run, Some(&diffs));
    assert_eq!(names(&list), ["src/toggle.rs"]);
    assert_eq!(list[0].stat.as_deref(), Some("+1 −1"));
}

#[test]
fn an_empty_diff_falls_back_to_the_reported_files() {
    let run = run_with(&["src/toggle.rs"]);
    let list = changed_file_list(&run, Some(&[]));
    assert_eq!(names(&list), ["src/toggle.rs"]);
    assert!(list[0].stat.is_none());
}
