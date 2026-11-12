use super::*;

#[tokio::test]
async fn pruefung_aus_dem_container_nennt_vpn_wenn_nur_der_mac_den_proxy_erreicht() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let proxy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[{"id":"claude-sonnet-5-5"}]})),
        )
        .mount(&proxy)
        .await;
    let w = welt(true);
    // Der Proxy des Tests liegt auf dem Mac; der „Container“ erreicht ihn nicht.
    let e = Einstellungen {
        agent: AgentWahl {
            anbieter: Provider::ClaudeAgentSdk,
            endpunkt: Some(EndpunktKonfig {
                url: Some(proxy.uri()),
                modell: Some("claude-sonnet-5-5".into()),
            }),
        },
        ..w.state.inner.einstellungen.clone()
    };
    let conn = crate::db::open(&w.tmp.path().join("p.db")).unwrap();
    let state = AppState::neu(
        conn,
        w.secrets.clone(),
        8787,
        crate::abgleich::Quellen::default(),
        e,
    );
    w.schreibe(
        "szenario-plan.sh",
        r#"printf '%s\n' '{"t":"pruefung","erreichbar":false,"key_ok":false,"hinweis":"Der Container erreicht den Proxy nicht (ENOTFOUND)."}'
exit 1
"#,
    );
    let p = state.container_pruefen().await;
    assert!(p.checked && p.mac_reached && !p.container_reached, "{p:?}");
    let h = p.hint.unwrap();
    assert!(
        h.contains("Dein Mac erreicht den Proxy, der Container aber nicht")
            && h.contains("VPN")
            && h.contains("ENOTFOUND"),
        "{h}"
    );
    let aufrufe = w.log("aufrufe.log");
    let z = aufrufe.lines().find(|z| z.starts_with("run ")).unwrap();
    assert!(z.contains("--rm") && z.contains(":ro"), "{z}");
    assert!(!aufrufe.contains(TOKEN));
    // Läuft er auch im Container, ist alles gut.
    w.schreibe(
        "szenario-plan.sh",
        "printf '%s\\n' '{\"t\":\"pruefung\",\"erreichbar\":true,\"key_ok\":true,\"hinweis\":\"ok\"}'\nexit 0\n",
    );
    let p = state.container_pruefen().await;
    assert!(p.container_reached && p.key_ok);
    assert!(
        p.hint
            .unwrap()
            .contains("auch aus dem Container erreichbar")
    );
}

#[tokio::test]
async fn pruefung_aus_dem_container_ohne_image_oder_key_ruft_nichts_auf() {
    let w = welt(false);
    let p = w.state.container_pruefen().await;
    assert!(!p.checked && p.hint.unwrap().contains("nicht eingerichtet"));
    let w = welt(true);
    std::fs::write(w.bin().join("kein-image"), "").unwrap();
    let p = w.state.container_pruefen().await;
    assert!(!p.checked && p.hint.unwrap().contains("Image fehlt"));
    assert!(!w.log("aufrufe.log").contains("run "));
}
