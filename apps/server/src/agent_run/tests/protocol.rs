use super::*;

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
        assert!(text.contains(muss), "{muss} is missing in {text}");
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
