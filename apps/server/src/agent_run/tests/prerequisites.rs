use super::*;
use cockpit_i18n::Msg;

/// The German texts of the missing parts.
fn texts(f: Vec<Msg>) -> Vec<String> {
    f.iter().map(Msg::de).collect()
}

#[test]
fn prerequisites_say_in_plain_text_was_missing_and_where() {
    let no = ContainerStatus::default();
    let v = Prerequisites {
        container: &no,
        image: ImageState::Missing,
        image_hint: None,
        model: false,
        key: false,
        github_access: false,
    };
    let f = texts(missing(&v));
    assert_eq!(
        f.len(),
        3,
        "no repo is needed to be ready: it is chosen at the start"
    );
    assert!(f[0].contains("Einrichten") && f[0].contains("Container"));
    assert!(f[1].contains("der Modellname, der Key") && !f[1].contains("Adresse"));
    assert!(f[2].contains("GitHub"));
    let running = ContainerStatus {
        runtime: Some("docker".into()),
        engine_running: true,
        ..ContainerStatus::default()
    };
    let fast = Prerequisites {
        container: &running,
        image: ImageState::Missing,
        image_hint: None,
        model: true,
        key: true,
        github_access: true,
    };
    let f = texts(missing(&fast));
    assert_eq!(f.len(), 1);
    assert!(f[0].contains("Image bauen"));
    // Outdated and not checkable images block a run too, with different advice.
    let outdated = Prerequisites {
        container: &running,
        image: ImageState::Outdated,
        image_hint: None,
        model: true,
        key: true,
        github_access: true,
    };
    let f = texts(missing(&outdated));
    assert_eq!(f.len(), 1);
    assert!(
        f[0].contains("veraltet") && f[0].contains("neu bauen"),
        "{f:?}"
    );
    let s = status(true, &outdated);
    assert!(!s.ready && s.image && s.image_state == ImageState::Outdated);
    let unknown = Prerequisites {
        image: ImageState::Unknown,
        image_hint: Some(Msg::raw("Der Ordner fehlt.")),
        ..outdated
    };
    let f = texts(missing(&unknown));
    assert!(
        f[0].contains("nicht prüfen") && f[0].contains("Der Ordner fehlt."),
        "{f:?}"
    );
    let s = status(
        true,
        &Prerequisites {
            image: ImageState::Current,
            image_hint: None,
            ..fast
        },
    );
    assert!(s.ready && s.mode == "real");
    let not_running = ContainerStatus {
        runtime: Some("podman".into()),
        engine_running: false,
        ..ContainerStatus::default()
    };
    let f = texts(missing(&Prerequisites {
        container: &not_running,
        image: ImageState::Current,
        image_hint: None,
        model: true,
        key: true,
        github_access: true,
    }));
    assert!(f[0].contains("Podman") && f[0].contains("Schritt „Container“"));
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
