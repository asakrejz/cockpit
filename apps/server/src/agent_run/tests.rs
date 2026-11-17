use super::*;
use cockpit_core::{Action, Actor, AgentOptions, demo};

const T: u64 = 1_700_000_000_000;

#[test]
fn lines_are_read_truncated_and_unknown_discarded() {
    let e = line_read(
        r#"{"t":"step","title":"Datei lesen","text":"a","tokens_in":10,"tokens_out":2,"ms":5}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Event::Step {
            title: "Datei lesen".into(),
            text: "a".into(),
            tokens_in: 10,
            tokens_out: 2,
            ms: 5
        }
    );
    assert_eq!(line_read("das ist kein json"), Err(Discarded::Unreadable));
    assert_eq!(line_read(r#"{"t":"rm -rf"}"#), Err(Discarded::Unreadable));
    assert_eq!(line_read(r#"{"t":"step"}"#), Err(Discarded::Unreadable));
    assert_eq!(line_read(""), Err(Discarded::Unreadable));
    let gross = format!(r#"{{"t":"error","text":"{}"}}"#, "x".repeat(MAX_LINE));
    assert_eq!(line_read(&gross), Err(Discarded::TooLarge));
    // Oversized values are limited, not taken over.
    let Ok(Event::Step {
        tokens_in,
        ms,
        title,
        ..
    }) = line_read(&format!(
        r#"{{"t":"step","title":"{}","tokens_in":99999999999999,"ms":99999999999}}"#,
        "y".repeat(5_000)
    ))
    else {
        panic!()
    };
    assert_eq!(tokens_in, MAX_TOKEN);
    assert_eq!(ms, MAX_MS);
    assert_eq!(title.chars().count(), 120);
    let Ok(Event::Files { files }) = line_read(&format!(
        r#"{{"t":"files","files":[{}]}}"#,
        vec![r#""a""#; 1_000].join(",")
    )) else {
        panic!()
    };
    assert_eq!(files.len(), MAX_LIST);
}

#[test]
fn events_are_only_in_matching_mode_to_messages() {
    let plan = Event::Plan {
        summary: "s".into(),
        files: vec![],
        steps: vec![],
    };
    assert!(as_message(&plan, Mode::Plan).is_some());
    assert!(as_message(&plan, Mode::Implement).is_none());
    let question = Event::Question {
        text: "?".into(),
        suggestions: vec![],
    };
    assert!(as_message(&question, Mode::Plan).is_none());
    assert!(as_message(&question, Mode::Implement).is_some());
    let error = Event::Error { text: "x".into() };
    assert!(as_message(&error, Mode::Implement).is_none());
}

#[test]
fn command_line_without_secrets_and_with_the_shared_switches() {
    let b = RunCommand {
        name: "cockpit-run-BAU-142-1",
        run: "BAU-142",
        clone: Path::new("/data/clone"),
        clone_only_read: false,
        job: Path::new("/data/job"),
        stdin_open: true,
        provider: Provider::ClaudeAgentSdk,
    };
    let a = run_args(&b);
    let text = a.join(" ");
    for muss in [
        "run --rm -i",
        "--label cockpit.run=BAU-142",
        "--add-host cockpit-host:host-gateway",
        "-v /data/clone:/work -v /data/job:/job:ro",
        "-e COCKPIT_PROVIDER -e ANTHROPIC_BASE_URL -e ANTHROPIC_AUTH_TOKEN -e ANTHROPIC_MODEL -e ANTHROPIC_API_KEY",
        "--cap-drop ALL",
    ] {
        assert!(text.contains(muss), "{muss} fehlt in {text}");
    }
    assert_eq!(a.last().unwrap(), IMAGE);
    // Fully named image, no SELinux addition, no --user (it is in the Containerfile).
    assert!(IMAGE.starts_with("localhost/"));
    assert!(
        !text.contains(":Z") && !text.contains("--user") && !text.contains("host.docker.internal")
    );
    // After `-e` there is only the name, never a `=` with a value.
    for (i, x) in a.iter().enumerate() {
        if x == "-e" {
            assert!(!a[i + 1].contains('='), "{}", a[i + 1]);
        }
    }
    // The values belong in the environment of the child process.
    let env = env_for_kind(
        Provider::ClaudeAgentSdk,
        Some("http://localhost:4000/"),
        "sk-geheim",
        "claude-sonnet-5-5",
    );
    assert!(env.contains(&(
        "ANTHROPIC_BASE_URL",
        Some("http://cockpit-host:4000".to_string())
    )));
    assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN", Some("sk-geheim".to_string()))));
    // With an address never an `ANTHROPIC_API_KEY` from the person's environment.
    assert!(env.contains(&("ANTHROPIC_API_KEY", None)));
    assert!(!text.contains("sk-geheim"));
    let ro = run_args(&RunCommand {
        clone_only_read: true,
        stdin_open: false,
        ..b
    });
    assert!(ro.join(" ").contains("/data/clone:/work:ro"));
    assert!(!ro.contains(&"-i".to_string()));
}

#[test]
fn direct_at_provider_gives_only_the_api_key_further() {
    for empty in [None, Some(""), Some("  ")] {
        let env = env_for_kind(
            Provider::ClaudeAgentSdk,
            empty,
            "sk-ant-geheim",
            "claude-opus-5-5",
        );
        assert!(env.contains(&("ANTHROPIC_API_KEY", Some("sk-ant-geheim".to_string()))));
        assert!(env.contains(&("ANTHROPIC_BASE_URL", None)));
        assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN", None)));
        assert!(env.contains(&("ANTHROPIC_MODEL", Some("claude-opus-5-5".to_string()))));
    }
    // Every name that we set for the process is also in the `-e` list, and the other family is only removed.
    for p in Provider::ALL {
        let names = kind_env_names(p);
        for (n, v) in env_for_kind(p, None, "k", "m") {
            if v.is_some() {
                assert!(names.contains(&n), "{p:?} {n}");
            }
        }
    }
}

#[test]
fn every_tool_gets_its_adapter_and_the_names_of_its_endpoint_family() {
    for p in [Provider::ClaudeAgentSdk, Provider::ClaudeCode] {
        let env = env_for_kind(p, Some("http://localhost:4000"), "sk-geheim", "m");
        assert!(env.contains(&("COCKPIT_PROVIDER", Some(p.id().to_string()))));
        assert!(env.contains(&("ANTHROPIC_AUTH_TOKEN", Some("sk-geheim".into()))));
        assert!(env.contains(&("ANTHROPIC_MODEL", Some("m".into()))));
        for gone in ["OPENAI_API_KEY", "OPENAI_BASE_URL", "COCKPIT_MODEL"] {
            assert!(env.contains(&(gone, None)), "{gone}");
        }
        assert!(!kind_env_names(p).contains(&"OPENAI_API_KEY"));
    }
    // Codex: bearer key in OPENAI_API_KEY, address with the local machine renamed, model in COCKPIT_MODEL.
    let env = env_for_kind(
        Provider::Codex,
        Some("http://127.0.0.1:4000/"),
        "sk-geheim",
        "gpt-x",
    );
    assert!(env.contains(&("COCKPIT_PROVIDER", Some("codex".into()))));
    assert!(env.contains(&("OPENAI_API_KEY", Some("sk-geheim".into()))));
    assert!(env.contains(&("OPENAI_BASE_URL", Some("http://cockpit-host:4000".into()))));
    assert!(env.contains(&("COCKPIT_MODEL", Some("gpt-x".into()))));
    for gone in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
    ] {
        assert!(env.contains(&(gone, None)), "{gone}");
    }
    // Directly at OpenAI: no base address.
    let direct = env_for_kind(Provider::Codex, None, "k", "m");
    assert!(direct.contains(&("OPENAI_BASE_URL", None)));
    let args = run_args(&RunCommand {
        name: "n",
        run: "r",
        clone: Path::new("/c"),
        clone_only_read: false,
        job: Path::new("/j"),
        stdin_open: false,
        provider: Provider::Codex,
    })
    .join(" ");
    assert!(
        args.contains("-e COCKPIT_PROVIDER -e OPENAI_BASE_URL -e OPENAI_API_KEY -e COCKPIT_MODEL"),
        "{args}"
    );
    assert!(!args.contains("ANTHROPIC") && !args.contains("k=") && !args.contains("sk-"));
}

#[test]
fn proxy_on_the_machine_is_called_in_container_cockpit_host() {
    for (ein, aus) in [
        ("http://localhost:4000", "http://cockpit-host:4000"),
        ("http://127.0.0.1:4000/", "http://cockpit-host:4000"),
        ("https://localhost/v1", "https://cockpit-host/v1"),
        ("http://[::1]:4000", "http://cockpit-host:4000"),
        ("https://litellm.example", "https://litellm.example"),
        (
            "https://litellm.example:8443/x/",
            "https://litellm.example:8443/x",
        ),
        ("https://localhost.example", "https://localhost.example"),
    ] {
        assert_eq!(for_container(ein), aus, "{ein}");
    }
}

#[test]
fn names_are_clean() {
    assert_eq!(container_name("BAU-142", 7), "cockpit-run-BAU-142-7");
    assert_eq!(container_name("a b/#1", 7), "cockpit-run-a-b--1-7");
    assert_eq!(run_dir("a b/#1"), "a_b__1");
}

fn start(ws: &mut Workspace, plan_first: bool) {
    ws.apply(
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions {
                plan_first,
                ..AgentOptions::default()
            },
        },
        Actor::User,
        T,
    )
    .unwrap();
}

fn info(mode: Mode) -> Info {
    Info {
        mode,
        paused: false,
        question_open: false,
        hints_sent: 0,
    }
}

fn running(m: Mode) -> HashMap<String, Info> {
    HashMap::from([("BAU-142".to_string(), info(m))])
}

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
    let run = ws.run("threads-asakrejz~rust-playground-85").unwrap();
    // The watcher starts the container for the real follow-up job like for a normal run.
    assert_eq!(
        plane(&ws, &HashMap::new(), &[]),
        [Instruction::Start(
            "threads-asakrejz~rust-playground-85".into(),
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
fn watcher_starts_plan_and_run_je_after_state() {
    let mut ws = demo::workspace();
    assert!(plane(&ws, &HashMap::new(), &[]).is_empty());
    start(&mut ws, true);
    assert_eq!(
        plane(&ws, &HashMap::new(), &[]),
        [Instruction::Start("BAU-142".into(), Mode::Plan)]
    );
    // With a running plan container nothing happens until the plan is there.
    assert!(plane(&ws, &running(Mode::Plan), &[]).is_empty());
    ws.report_agent(
        "BAU-142",
        AgentReport::Plan {
            summary: "s".into(),
            files: vec![],
            steps: vec![],
        },
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &running(Mode::Plan), &[]),
        [Instruction::Stop("BAU-142".into())],
        "ein fertiger Plan-Container wird nicht weiter gebraucht"
    );
    assert!(
        plane(&ws, &HashMap::new(), &[]).is_empty(),
        "wartet auf die Freigabe"
    );
    ws.apply(
        Action::ApprovePlan {
            ticket: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &HashMap::new(), &[]),
        [Instruction::Start("BAU-142".into(), Mode::Implement)]
    );
    // A run that has just failed does not restart immediately.
    assert!(plane(&ws, &HashMap::new(), &["BAU-142".to_string()]).is_empty());
}

#[test]
fn watcher_stops_at_cancel_limit_and_orphaned_containern() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    assert!(plane(&ws, &running(Mode::Implement), &[]).is_empty());
    let mut stop = ws.clone();
    stop.apply(
        Action::CancelAgent {
            ticket: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&stop, &running(Mode::Implement), &[]),
        [Instruction::Stop("BAU-142".into())]
    );
    // Limit reached: halted.
    let mut limit = ws.clone();
    let mut l = limit.limits.clone();
    l.run_budget_cents = Some(1);
    limit.set_limits(l);
    limit
        .report_agent(
            "BAU-142",
            AgentReport::Step {
                title: "t".into(),
                text: String::new(),
                input_tokens: 1_000_000,
                output_tokens: 0,
                millis: 1,
            },
            T,
        )
        .unwrap();
    assert_eq!(
        plane(&limit, &running(Mode::Implement), &[]),
        [Instruction::Stop("BAU-142".into())]
    );
    // Container for a run that does not exist.
    let empty = demo::workspace();
    assert_eq!(
        plane(&empty, &running(Mode::Implement), &[]),
        [Instruction::Stop("BAU-142".into())]
    );
}

#[test]
fn watcher_passes_pause_answer_and_hints_further() {
    let mut ws = demo::workspace();
    start(&mut ws, false);
    ws.apply(
        Action::PauseAgent {
            run: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &running(Mode::Implement), &[]),
        [Instruction::Pause("BAU-142".into())]
    );
    ws.apply(
        Action::ResumeAgent {
            run: "BAU-142".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    let mut i = info(Mode::Implement);
    i.paused = true;
    let mut m = HashMap::from([("BAU-142".to_string(), i)]);
    assert_eq!(plane(&ws, &m, &[]), [Instruction::Resume("BAU-142".into())]);

    // Question and answer.
    ws.report_agent(
        "BAU-142",
        AgentReport::Question {
            text: "Welcher?".into(),
            suggestions: vec![],
        },
        T,
    )
    .unwrap();
    m.get_mut("BAU-142").unwrap().paused = false;
    m.get_mut("BAU-142").unwrap().question_open = true;
    assert!(plane(&ws, &m, &[]).is_empty(), "solange nicht beantwortet");
    ws.apply(
        Action::AnswerAgent {
            run: "BAU-142".into(),
            text: "der blaue".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &m, &[]),
        [Instruction::Answer("BAU-142".into(), "der blaue".into())]
    );
    m.get_mut("BAU-142").unwrap().question_open = false;

    // Hints go out exactly once.
    ws.apply(
        Action::SteerAgent {
            run: "BAU-142".into(),
            text: "Kleine Commits".into(),
        },
        Actor::User,
        T,
    )
    .unwrap();
    assert_eq!(
        plane(&ws, &m, &[]),
        [Instruction::Hints(
            "BAU-142".into(),
            vec!["Kleine Commits".into()]
        )]
    );
    m.get_mut("BAU-142").unwrap().hints_sent = 1;
    assert!(plane(&ws, &m, &[]).is_empty());
}

#[test]
fn prerequisites_say_in_plain_text_was_missing_and_where() {
    let no = ContainerStatus::default();
    let v = Prerequisites {
        container: &no,
        image: false,
        model: false,
        key: false,
        repo: Err("Es ist kein Repo eingetragen.".into()),
        github_access: false,
    };
    let f = missing(&v);
    assert_eq!(f.len(), 4);
    assert!(f[0].contains("Einrichten") && f[0].contains("Container"));
    assert!(f[1].contains("der Modellname, der Key") && !f[1].contains("Adresse"));
    assert!(f[3].contains("GitHub"));
    let running = ContainerStatus {
        runtime: Some("docker".into()),
        engine_running: true,
        ..ContainerStatus::default()
    };
    let fast = Prerequisites {
        container: &running,
        image: false,
        model: true,
        key: true,
        repo: Ok("firma/app"),
        github_access: true,
    };
    let f = missing(&fast);
    assert_eq!(f.len(), 1);
    assert!(f[0].contains("Image bauen"));
    let s = status(
        true,
        &Prerequisites {
            image: true,
            ..fast
        },
    );
    assert!(s.ready && s.mode == "real" && s.repo.as_deref() == Some("firma/app"));
    let not_running = ContainerStatus {
        runtime: Some("podman".into()),
        engine_running: false,
        ..ContainerStatus::default()
    };
    let f = missing(&Prerequisites {
        container: &not_running,
        image: true,
        model: true,
        key: true,
        repo: Ok("a/b"),
        github_access: true,
    });
    assert!(f[0].contains("podman machine start"));
    // Simulated: always ready.
    assert!(status(false, &v).ready);
}

#[test]
fn the_repos_the_agents() {
    let one = vec!["a/b".to_string()];
    let two = vec!["a/b".to_string(), "c/d".to_string()];
    let listed = vec![
        AgentRepo::new("a/b"),
        AgentRepo {
            projects: vec!["WEB".into()],
            ..AgentRepo::new("c/d")
        },
    ];
    // Only the default: it is the only repo, whatever the sync repos are.
    let r = repos_effective(&[], Some("x/y"), &two);
    assert_eq!((r.repos.len(), r.default.as_deref()), (1, Some("x/y")));
    // Only one sync repo: it is the default.
    let r = repos_effective(&[], None, &one);
    assert_eq!(r.default.as_deref(), Some("a/b"));
    // Several sync repos and no default: all are choices, the user picks.
    let r = repos_effective(&[], None, &two);
    assert_eq!((r.repos.len(), r.default), (2, None));
    // The list plus a default that is not in it.
    let r = repos_effective(&listed, Some("e/f"), &[]);
    assert_eq!(r.repos.len(), 3);
    assert_eq!(r.default.as_deref(), Some("e/f"));
    // Nothing at all.
    assert!(repos_effective(&[], None, &[]).is_empty());
}
