use super::*;

#[test]
fn ids_and_options_agree() {
    for id in Filter::IDS {
        assert_eq!(Filter::from_id(id).map(Filter::id), Some(*id));
    }
    for id in TicketStatus::IDS {
        assert_eq!(TicketStatus::from_id(id).map(TicketStatus::id), Some(*id));
    }
    for id in SourceFilter::IDS {
        assert_eq!(SourceFilter::from_id(id).map(SourceFilter::id), Some(*id));
    }
    assert_eq!(Filter::from_id("???"), None);
    assert_eq!(SourceFilter::from_id("???"), None);
}

#[test]
fn filter_matches_scope() {
    assert!(Filter::All.matches(Scope::Mine) && Filter::All.matches(Scope::Team));
    assert!(Filter::Mine.matches(Scope::Mine) && !Filter::Mine.matches(Scope::Team));
    assert!(Filter::Team.matches(Scope::Team) && !Filter::Team.matches(Scope::Mine));
}

#[test]
fn source_filter_matches_source() {
    assert!(SourceFilter::All.matches(Source::Jira) && SourceFilter::All.matches(Source::GitHub));
    assert!(
        SourceFilter::Jira.matches(Source::Jira) && !SourceFilter::Jira.matches(Source::GitHub)
    );
    assert!(
        SourceFilter::GitHub.matches(Source::GitHub) && !SourceFilter::GitHub.matches(Source::Jira)
    );
}

#[test]
fn priorities_roundtrip_and_default_is_medium() {
    for id in Priority::IDS {
        assert_eq!(Priority::from_id(id).map(Priority::id), Some(*id));
    }
    assert_eq!(Priority::from_id("???"), None);
    assert_eq!(Priority::default(), Priority::Medium);
    assert_eq!(Priority::IDS.len(), Priority::ALL.len());
}

#[test]
fn pr_state_suggests_ticket_status() {
    let mut pr = PrRef {
        repo: String::new(),
        number: 1,
        draft: true,
        checks: Checks::Passed,
        changes_requested: false,
        merged: false,
        review_requested: false,
        author: String::new(),
        mine: false,
        requested_me: false,
        assigned_me: false,
    };
    assert_eq!(pr.target_status(), TicketStatus::InProgress);
    pr.draft = false;
    assert_eq!(pr.target_status(), TicketStatus::InProgress, "offen");
    pr.review_requested = true;
    assert_eq!(pr.target_status(), TicketStatus::Review);
    pr.merged = true;
    assert_eq!(pr.target_status(), TicketStatus::Done);
}

#[test]
fn pr_keys_read_and_write_the_old_and_the_new_wire_form() {
    // Old form: a plain number; new form: `owner/name#12`.
    assert_eq!(serde_json::to_string(&PrKey::from(87)).unwrap(), "87");
    let key = PrKey::new("acme/app", 87);
    assert_eq!(serde_json::to_string(&key).unwrap(), "\"acme/app#87\"");
    assert_eq!(
        serde_json::from_str::<PrKey>("87").unwrap(),
        PrKey::from(87)
    );
    assert_eq!(
        serde_json::from_str::<PrKey>("\"acme/app#87\"").unwrap(),
        key
    );
    assert_eq!(
        serde_json::from_str::<PrKey>(r#"{"repo":"acme/app","number":87}"#).unwrap(),
        key
    );
    assert!(serde_json::from_str::<PrKey>("\"nonsense\"").is_err());
}

#[test]
fn pr_keys_match_by_repo_unless_a_repo_is_unknown() {
    let app = PrKey::new("acme/app", 5);
    assert!(app.matches("acme/app", 5));
    assert!(!app.matches("acme/web", 5));
    assert!(!app.matches("acme/app", 6));
    assert!(app.matches("", 5), "an entry of an old state without repo");
    assert!(
        PrKey::from(5).matches("acme/web", 5),
        "a plain number from an old client"
    );
    assert_eq!(app.to_string(), "acme/app#5");
    assert_eq!(PrKey::from(5).to_string(), "#5");
}

#[test]
fn pr_keys_make_hash_addresses_and_safe_names() {
    let key = PrKey::new("acme/app", 5);
    assert_eq!(key.route(), "acme/app/5");
    assert_eq!(PrKey::from_route("acme/app/5"), Some(key.clone()));
    assert_eq!(PrKey::from_route("5"), Some(PrKey::from(5)));
    assert_eq!(PrKey::from_route("abc"), None);
    assert_eq!(PrKey::from_route("x/5"), None, "not owner/name");
    assert_eq!(key.slug(), "acme~app-5");
    assert_ne!(PrKey::new("a-b/c", 1).slug(), PrKey::new("a/b-c", 1).slug());
}

#[test]
fn a_run_belongs_to_the_pull_request_entry_it_names() {
    let mut item = crate::demo::workspace().item_by_key("#87").unwrap().clone();
    let repo = item.pr.as_ref().unwrap().repo.clone();
    assert!(item.matches_run_key("#87"));
    assert!(item.matches_run_key(&format!("{repo}#87")));
    assert!(!item.matches_run_key("other/repo#87"));
    item.pr.as_mut().unwrap().number = 9;
    assert!(!item.matches_run_key(&format!("{repo}#87")));
}

#[test]
fn pr_status_tones() {
    let pr = PrRef {
        repo: String::new(),
        number: 87,
        draft: false,
        checks: Checks::Passed,
        changes_requested: false,
        merged: false,
        review_requested: false,
        author: String::new(),
        mine: false,
        requested_me: false,
        assigned_me: false,
    };
    assert_eq!(pr.status_tone(), Tone::Orange);
    let failed = PrRef {
        repo: String::new(),
        number: 85,
        draft: true,
        checks: Checks::Failed,
        changes_requested: false,
        merged: false,
        review_requested: false,
        author: String::new(),
        mine: false,
        requested_me: false,
        assigned_me: false,
    };
    assert_eq!(failed.status_tone(), Tone::Red);
    let merged = PrRef { merged: true, ..pr };
    assert_eq!(merged.status_tone(), Tone::Indigo);
}
