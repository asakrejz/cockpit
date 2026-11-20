use super::*;

#[test]
fn the_job_carries_ticket_rules_and_branch_but_no_secrets() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    let run = ws.run("BAU-142").unwrap();
    let j = job_json(&ws, &run, Mode::Implement, "claude/BAU-142-x", None);
    assert_eq!(j["mode"], "implement");
    assert_eq!(j["ticket"]["key"], "BAU-142");
    assert_eq!(j["branch"], "claude/BAU-142-x");
    assert_eq!(j["base_branch"], "main");
    let text = j.to_string();
    assert!(!text.contains("TOKEN") && !text.contains("sk-"));
    assert_eq!(check_job()["mode"], "check");
}

#[test]
fn the_job_names_the_repo_and_the_base_branch_of_the_run() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    let mut run = ws.run("BAU-142").unwrap();
    run.repo = "firma/web".into();
    run.options.base_branch = "develop".into();
    let j = job_json(&ws, &run, Mode::Implement, "claude/BAU-142-x", None);
    assert_eq!(j["repo"], "firma/web");
    assert_eq!(j["base_branch"], "develop");
}

#[test]
fn a_followup_job_carries_its_input_and_still_no_secrets() {
    let mut ws = demo::workspace();
    for r in ws.reviews.iter_mut().filter(|r| r.number == 85) {
        r.branch = "claude/BAU-127-x".into();
    }
    ws.apply(
        Action::AddressReviewThreads {
            review: 85.into(),
            options: AgentOptions {
                plan_first: false,
                ..AgentOptions::default()
            },
        },
        Actor::User,
        T,
    )
    .unwrap();
    let run = ws.run("threads-acme~app-85").unwrap();
    // The watcher starts the container for the real follow-up job like for a normal run.
    assert_eq!(
        plane(&ws, &HashMap::new(), &[]),
        [Instruction::Start(
            "threads-acme~app-85".into(),
            Mode::Implement
        )]
    );
    let job = FollowupJob {
        kind: "review_comments",
        review: 85,
        threads: vec![followup::Thread {
            path: "a.rs".into(),
            line: Some(3),
            comments: vec![followup::ThreadComment {
                author: "mira".into(),
                text: "Bitte prüfen".into(),
            }],
        }],
        ..FollowupJob::default()
    };
    let j = job_json(&ws, &run, Mode::Implement, &run.branch(), Some(&job));
    assert_eq!(j["branch"], "claude/BAU-127-x");
    assert_eq!(j["followup"]["kind"], "review_comments");
    assert_eq!(j["followup"]["threads"][0]["comments"][0]["author"], "mira");
    let text = j.to_string();
    assert!(!text.contains("TOKEN") && !text.contains("sk-") && !text.contains("ghp_"));
    // A normal run has no follow-up input.
    let mut ws = demo::workspace();
    start(&mut ws, false);
    let run = ws.run("BAU-142").unwrap();
    assert!(job_json(&ws, &run, Mode::Implement, "claude/x", None)["followup"].is_null());
}

#[test]
fn the_job_carries_the_interface_language_and_the_language_of_the_pull_request() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    let mut run = ws.run("BAU-142").unwrap();
    // Not chosen yet: English; the pull request language is English by default.
    let j = job_json(&ws, &run, Mode::Implement, "claude/BAU-142-x", None);
    assert_eq!(j["language"], "en");
    assert_eq!(j["pr_language"], "en");
    // The agent follows the language of the interface; the pull request follows the repo.
    ws.language = Some(cockpit_i18n::Lang::De);
    run.options.pr_language = cockpit_i18n::Lang::De;
    let j = job_json(&ws, &run, Mode::Implement, "claude/BAU-142-x", None);
    assert_eq!(j["language"], "de");
    assert_eq!(j["pr_language"], "de");
}
