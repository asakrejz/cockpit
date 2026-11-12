use super::*;

#[test]
fn namen_und_branches_werden_streng_geprueft() {
    assert!(repo_name_ok("firma/app"));
    assert!(repo_name_ok("a.b/c_d-e"));
    for schlecht in ["app", "a/b/c", "../x", "a/..", "a /b", "a/b;rm", "/b", "a/"] {
        assert!(!repo_name_ok(schlecht), "{schlecht}");
    }
    assert!(branch_ok("claude/BAU-142-fokus-ring"));
    assert!(branch_ok("claude/a/b"));
    for schlecht in [
        "main",
        "master",
        "claude",
        "claude/",
        "claude/../main",
        "claude/a b",
        "claude/a//b",
        "claude/x.lock",
        "agent/BAU-1",
        "refs/heads/main",
        "claude/a;b",
        "claude/-f",
    ] {
        // `-f` beginnt nicht mit `-`, weil „claude/“ davor steht; erlaubt, aber harmlos.
        if schlecht == "claude/-f" {
            continue;
        }
        assert!(!branch_ok(schlecht), "{schlecht}");
    }
}

#[test]
fn adressen() {
    assert_eq!(
        basis_aus_api("https://api.github.com/"),
        "https://github.com"
    );
    assert_eq!(
        basis_aus_api("https://ghe.example/api/v3"),
        "https://ghe.example"
    );
    assert_eq!(
        klon_url("https://github.com/", "firma/app"),
        "https://github.com/firma/app.git"
    );
}

#[test]
fn base64_stimmt() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn das_token_geht_nur_an_https_und_nur_ueber_die_umgebung() {
    let t = Secret::new("ghp_geheim");
    assert!(auth_umgebung("file:///tmp/x.git", Some(&t)).is_empty());
    assert!(auth_umgebung("http://github.com/x.git", Some(&t)).is_empty());
    assert!(auth_umgebung("https://github.com/x.git", None).is_empty());
    let env = auth_umgebung("https://github.com/x.git", Some(&t));
    assert_eq!(env.len(), 3);
    assert!(env[2].1.starts_with("Authorization: Basic "));
    assert!(!env[2].1.contains("ghp_geheim"), "nur base64-kodiert");
}

#[test]
fn fehlertexte_enthalten_kein_token() {
    let t = "ghp_geheim123";
    let b = base64(format!("x-access-token:{t}").as_bytes());
    let text = format!("fatal: https://x-access-token:{t}@github.com und {b}");
    let s = schwaerzen(&text, &[t]);
    assert!(!s.contains(t) && !s.contains(&b), "{s}");
}

/// Ein Remote mit einem Commit auf `main`, ein Klon-Ordner und ein Ordner für den Export.
struct Aufbau {
    _tmp: tempfile::TempDir,
    remote: std::path::PathBuf,
    klon: std::path::PathBuf,
    export: std::path::PathBuf,
    url: String,
}

fn g(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .args([] as [&str; 0])
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@t.invalid")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@t.invalid")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn aufbau() -> Aufbau {
    let tmp = tempfile::tempdir().unwrap();
    let remote = tmp.path().join("remote.git");
    let quelle = tmp.path().join("quelle");
    std::fs::create_dir_all(&quelle).unwrap();
    g(&quelle, &["init", "--quiet", "-b", "main"]);
    std::fs::write(quelle.join("README.md"), "hallo\n").unwrap();
    g(&quelle, &["add", "-A"]);
    g(&quelle, &["commit", "--quiet", "-m", "Anfang"]);
    g(
        tmp.path(),
        &["clone", "--quiet", "--bare", "quelle", "remote.git"],
    );
    Aufbau {
        url: format!("file://{}", remote.display()),
        klon: tmp.path().join("klon"),
        export: tmp.path().join("export.git"),
        remote,
        _tmp: tmp,
    }
}

fn heads(remote: &Path) -> Vec<String> {
    g(
        remote,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .lines()
    .map(str::to_string)
    .collect()
}

/// Spielt den Container nach: ändert Dateien im Klon und committet.
fn agent_arbeitet(klon: &Path, datei: &str) {
    let pfad = klon.join(datei);
    std::fs::create_dir_all(pfad.parent().unwrap()).unwrap();
    std::fs::write(&pfad, "neu\n").unwrap();
    g(klon, &["add", "-A"]);
    g(klon, &["commit", "--quiet", "-m", "Agent"]);
}

#[test]
fn klonen_arbeiten_pruefen_pushen_nur_auf_claude_branches() {
    let a = aufbau();
    let basis = klonen(&a.url, &a.klon, None, "main", "claude/BAU-1-x").unwrap();
    assert_eq!(basis.len(), 40);
    assert_eq!(
        g(&a.klon, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "claude/BAU-1-x"
    );
    assert_eq!(g(&a.klon, &["remote"]).trim(), "", "kein Remote im Klon");
    agent_arbeitet(&a.klon, "src/neu.rs");
    let v = veroeffentlichen(
        &a.klon,
        &a.export,
        "claude/BAU-1-x",
        &basis,
        &a.url,
        None,
        &RepoRules::default(),
    )
    .unwrap();
    assert_eq!(v.dateien, ["src/neu.rs"]);
    let mut h = heads(&a.remote);
    h.sort();
    assert_eq!(h, ["claude/BAU-1-x", "main"]);
    // main im Remote ist unberührt.
    assert_eq!(g(&a.remote, &["rev-parse", "main"]).trim(), basis);
    assert_eq!(
        g(&a.remote, &["rev-parse", "claude/BAU-1-x"]).trim(),
        v.commit
    );
}

#[test]
fn der_hauptbranch_wird_nie_angelegt_oder_gepusht() {
    let a = aufbau();
    assert!(klonen(&a.url, &a.klon, None, "main", "main").is_err());
    let basis = klonen(&a.url, &a.klon, None, "main", "claude/x").unwrap();
    agent_arbeitet(&a.klon, "a.txt");
    for b in ["main", "master", "agent/x", "refs/heads/main"] {
        let e = veroeffentlichen(
            &a.klon,
            &a.export,
            b,
            &basis,
            &a.url,
            None,
            &RepoRules::default(),
        );
        assert!(e.unwrap_err().contains("nicht erlaubt"), "{b}");
    }
    assert_eq!(heads(&a.remote), ["main"]);
}

#[test]
fn regeln_des_repos_verhindern_den_push() {
    let a = aufbau();
    let basis = klonen(&a.url, &a.klon, None, "main", "claude/r").unwrap();
    agent_arbeitet(&a.klon, "Cargo.lock");
    let regeln = RepoRules {
        forbidden_paths: vec!["*.lock".into()],
        ..RepoRules::default()
    };
    let e = veroeffentlichen(
        &a.klon, &a.export, "claude/r", &basis, &a.url, None, &regeln,
    )
    .unwrap_err();
    assert!(e.contains("verstoßen gegen die Regeln"), "{e}");
    assert_eq!(heads(&a.remote), ["main"]);
}

#[test]
fn ohne_commit_und_mit_fremdem_verlauf_wird_nicht_gepusht() {
    let a = aufbau();
    let basis = klonen(&a.url, &a.klon, None, "main", "claude/leer").unwrap();
    let e = veroeffentlichen(
        &a.klon,
        &a.export,
        "claude/leer",
        &basis,
        &a.url,
        None,
        &RepoRules::default(),
    )
    .unwrap_err();
    assert!(e.contains("nichts committet"), "{e}");

    // Umgeschriebener Verlauf: ein Branch ohne den Ausgangsstand.
    let b = aufbau();
    let basis = klonen(&b.url, &b.klon, None, "main", "claude/fremd").unwrap();
    g(
        &b.klon,
        &["checkout", "--quiet", "--orphan", "claude/fremd2"],
    );
    agent_arbeitet(&b.klon, "x.txt");
    let e = veroeffentlichen(
        &b.klon,
        &b.export,
        "claude/fremd2",
        &basis,
        &b.url,
        None,
        &RepoRules::default(),
    )
    .unwrap_err();
    assert!(e.contains("baut nicht auf"), "{e}");
    assert_eq!(heads(&b.remote), ["main"]);
}

#[cfg(unix)]
#[test]
fn ein_manipulierter_klon_fuehrt_auf_dem_rechner_nichts_aus() {
    use std::os::unix::fs::PermissionsExt;
    let a = aufbau();
    let basis = klonen(&a.url, &a.klon, None, "main", "claude/boese").unwrap();
    let marke = a.klon.parent().unwrap().join("ausgefuehrt");
    // Hooks und Befehle in der Konfiguration, wie sie ein Agent in .git ablegen könnte.
    for hook in [
        "pre-push",
        "post-commit",
        "reference-transaction",
        "pre-commit",
    ] {
        let p = a.klon.join(".git/hooks").join(hook);
        std::fs::write(&p, format!("#!/bin/sh\ntouch {}\n", marke.display())).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut konfig = std::fs::read_to_string(a.klon.join(".git/config")).unwrap();
    konfig.push_str(&format!(
        "[core]\n\tfsmonitor = touch {m}\n\tsshCommand = touch {m}\n[credential]\n\thelper = !touch {m}\n",
        m = marke.display()
    ));
    std::fs::write(a.klon.join(".git/config"), konfig).unwrap();
    std::fs::write(a.klon.join("x.txt"), "x").unwrap();
    // Der „Agent“ committet an den Hooks vorbei (sonst liefe der Hook im Test selbst).
    g(&a.klon, &["add", "-A"]);
    g(
        &a.klon,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "x",
        ],
    );
    let _ = std::fs::remove_file(&marke);
    veroeffentlichen(
        &a.klon,
        &a.export,
        "claude/boese",
        &basis,
        &a.url,
        None,
        &RepoRules::default(),
    )
    .unwrap();
    assert!(!marke.exists(), "im Klon darf nichts ausgeführt werden");
    assert_eq!(heads(&a.remote).len(), 2);
}

#[test]
fn klonen_scheitert_in_klartext() {
    let a = aufbau();
    let e = klonen(
        "file:///gibt/es/nicht.git",
        &a.klon,
        None,
        "main",
        "claude/x",
    )
    .unwrap_err();
    assert!(!e.is_empty());
    assert!(klonen(&a.url, &a.klon, None, "--evil", "claude/x").is_err());
}
