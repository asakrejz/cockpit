use super::*;

#[tokio::test]
async fn neustart_raeumt_verwaiste_container_auf_und_markiert_laeufe() {
    let w = welt(true);
    w.schreibe(
        "szenario-lauf.sh",
        "while [ ! -e \"$d/gestoppt-$name\" ]; do sleep 0.05; done\nexit 137\n",
    );
    w.start(false).await;
    w.warte("Container", |_| w.log("aufrufe.log").contains("run "))
        .await;
    // „Neustart“: neuer Server-Zustand mit demselben gespeicherten Stand, Container sind weg.
    let ws = w.ws();
    let conn = crate::db::open(&w.tmp.path().join("neu.db")).unwrap();
    let neu = AppState::neu(
        conn,
        w.secrets.clone(),
        8787,
        crate::abgleich::Quellen::default(),
        w.state.inner.einstellungen.clone(),
    );
    neu.inner.live.setze_stand_fuer_test(ws);
    w.schreibe("ps.out", "abc123\ndef456\n");
    let n = neu.clone();
    tokio::task::spawn_blocking(move || n.agent_aufraeumen())
        .await
        .unwrap();
    let aufrufe = w.log("aufrufe.log");
    assert!(
        aufrufe.contains("ps -a -q --filter label=cockpit.lauf"),
        "{aufrufe}"
    );
    assert!(aufrufe.contains("rm -f abc123 def456"), "{aufrufe}");
    let run = neu.inner.live.kopie().run("BAU-142").unwrap();
    assert!(run.is_idle());
    assert!(run.ended.unwrap().contains("Server wurde neu gestartet"));
    // Den alten Lauf im ersten Zustand beenden.
    w.aktion(Action::CancelAgent {
        ticket: "BAU-142".into(),
    })
    .await
    .unwrap();
    w.warte_auf_ende().await;
}

#[tokio::test]
async fn bau_des_images_mit_fortschritt_und_fehler() {
    let w = welt(true);
    w.schreibe(
        "build.out",
        "Step 1/5 : FROM docker.io/library/node:24-bookworm-slim\nStep 2/5 : RUN apt-get update\n",
    );
    let st = w.state.clone();
    let b = tokio::task::spawn_blocking(move || st.image_bauen())
        .await
        .unwrap()
        .unwrap();
    assert!(b.running);
    let ende = Instant::now() + Duration::from_secs(10);
    loop {
        let st = w.state.clone();
        let b = tokio::task::spawn_blocking(move || st.image_stand())
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
        assert!(Instant::now() < ende);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let aufrufe = w.log("aufrufe.log");
    let bau = aufrufe.lines().find(|z| z.starts_with("build ")).unwrap();
    assert!(
        bau.contains("-f ")
            && bau.contains("Containerfile")
            && bau.contains("-t localhost/cockpit-agent:1"),
        "{bau}"
    );

    // Fehlschlag mit Rat.
    w.schreibe("build.fehler", "");
    let st = w.state.clone();
    tokio::task::spawn_blocking(move || st.image_bauen())
        .await
        .unwrap()
        .unwrap();
    let ende = Instant::now() + Duration::from_secs(10);
    loop {
        let st = w.state.clone();
        let b = tokio::task::spawn_blocking(move || st.image_stand())
            .await
            .unwrap();
        if !b.running {
            let f = b.error.expect("Fehler");
            assert!(f.contains("gescheitert") && f.contains("Platz"), "{f}");
            break;
        }
        assert!(Instant::now() < ende);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn bau_ohne_containerfile_und_ohne_laufzeit_sagt_warum() {
    let w = welt(true);
    std::fs::remove_file(
        w.state
            .inner
            .einstellungen
            .agent_dir
            .clone()
            .unwrap()
            .join("Containerfile"),
    )
    .unwrap();
    let st = w.state.clone();
    let e = tokio::task::spawn_blocking(move || st.image_bauen())
        .await
        .unwrap()
        .unwrap_err();
    assert!(e.contains("Containerfile fehlt"), "{e}");
    // Ohne Container-Programm: neuer Zustand (der Zwischenspeicher gilt 10 Sekunden).
    let w = welt(true);
    std::fs::remove_file(w.bin().join("docker")).unwrap();
    let st = w.state.clone();
    let e = tokio::task::spawn_blocking(move || st.image_bauen())
        .await
        .unwrap()
        .unwrap_err();
    assert!(e.contains("docker") && e.contains("PATH"), "{e}");
}
