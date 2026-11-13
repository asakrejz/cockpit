use super::*;

#[tokio::test]
async fn restart_cleans_orphaned_container_on_and_marks_runs() {
    let w = welt(true);
    w.write(
        "scenario-run.sh",
        "while [ ! -e \"$d/stopped-$name\" ]; do sleep 0.05; done\nexit 137\n",
    );
    w.start(false).await;
    w.wait("Container", |_| w.log("calls.log").contains("run "))
        .await;
    // "Restart": new server state with the same stored state, containers are gone.
    let ws = w.ws();
    let conn = crate::db::open(&w.tmp.path().join("neu.db")).unwrap();
    let updated = AppState::create(
        conn,
        w.secrets.clone(),
        8787,
        crate::sync::Sources::default(),
        w.state.inner.settings.clone(),
    );
    updated.inner.live.set_state_for_test(ws);
    w.write("ps.out", "abc123\ndef456\n");
    let n = updated.clone();
    tokio::task::spawn_blocking(move || n.agent_clean_up())
        .await
        .unwrap();
    let calls = w.log("calls.log");
    assert!(
        calls.contains("ps -a -q --filter label=cockpit.run"),
        "{calls}"
    );
    assert!(calls.contains("rm -f abc123 def456"), "{calls}");
    let run = updated.inner.live.copy().run("BAU-142").unwrap();
    assert!(run.is_idle());
    assert!(run.ended.unwrap().contains("Server wurde neu gestartet"));
    // End the old run in the first state.
    w.action(Action::CancelAgent {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.wait_for_end().await;
}

#[tokio::test]
async fn build_the_images_with_progress_and_error() {
    let w = welt(true);
    w.write(
        "build.out",
        "Step 1/5 : FROM docker.io/library/node:24-bookworm-slim\nStep 2/5 : RUN apt-get update\n",
    );
    let st = w.state.clone();
    let b = tokio::task::spawn_blocking(move || st.image_build())
        .await
        .unwrap()
        .unwrap();
    assert!(b.running);
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let st = w.state.clone();
        let b = tokio::task::spawn_blocking(move || st.image_state())
            .await
            .unwrap();
        if !b.running {
            assert!(b.error.is_none(), "{b:?}");
            assert!(b.present);
            assert!(
                b.lines.iter().any(|z| z.starts_with("Step 2/5")),
                "{:?}",
                b.lines
            );
            break;
        }
        assert!(Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let calls = w.log("calls.log");
    let build = calls.lines().find(|z| z.starts_with("build ")).unwrap();
    assert!(
        build.contains("-f ")
            && build.contains("Containerfile")
            && build.contains("-t localhost/cockpit-agent:4"),
        "{build}"
    );

    // Failure with advice.
    w.write("build.error", "");
    let st = w.state.clone();
    tokio::task::spawn_blocking(move || st.image_build())
        .await
        .unwrap()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let st = w.state.clone();
        let b = tokio::task::spawn_blocking(move || st.image_state())
            .await
            .unwrap();
        if !b.running {
            let f = b.error.expect("Fehler");
            assert!(f.contains("gescheitert") && f.contains("Platz"), "{f}");
            break;
        }
        assert!(Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn build_without_containerfile_and_without_runtime_says_why() {
    let w = welt(true);
    std::fs::remove_file(
        w.state
            .inner
            .settings
            .agent_dir
            .clone()
            .unwrap()
            .join("Containerfile"),
    )
    .unwrap();
    let st = w.state.clone();
    let e = tokio::task::spawn_blocking(move || st.image_build())
        .await
        .unwrap()
        .unwrap_err();
    assert!(e.contains("Containerfile fehlt"), "{e}");
    // Without a container program: new state (the cache applies for 10 seconds).
    let w = welt(true);
    std::fs::remove_file(w.bin().join("docker")).unwrap();
    let st = w.state.clone();
    let e = tokio::task::spawn_blocking(move || st.image_build())
        .await
        .unwrap()
        .unwrap_err();
    assert!(e.contains("docker") && e.contains("PATH"), "{e}");
}
