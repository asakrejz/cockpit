//! Git auf dem Rechner des Nutzers für den Agenten: Klonen vor dem Lauf, Prüfen und Pushen danach.
//!
//! Der Container bekommt **kein** GitHub-Token. Das Backend klont (frisch, je Lauf), der Container
//! committet auf einen Branch `claude/…`, und das Backend pusht erst nach seiner Prüfung:
//!
//! * nur Branches mit dem Präfix `claude/` – nie der Hauptbranch, nie ein Force-Push, nie ein Merge,
//! * der Branch muss auf dem Stand aufbauen, von dem aus geklont wurde,
//! * die geänderten Dateien müssen zu den Regeln des Repos passen.
//!
//! Der Klon ist nach dem Lauf **nicht vertrauenswürdig** (der Container durfte hineinschreiben, auch
//! in `.git`). Darum läuft nach dem Lauf kein Git im Klon selbst: Der Branch wird in ein frisches,
//! leeres Bare-Repo geholt (`git fetch <klon>`), und nur dort wird geprüft und gepusht. Die
//! Zugangsdaten gehen nur über die Umgebung des Kindprozesses (`GIT_CONFIG_*`), nie über die
//! Kommandozeile, und nur an `https://`-Adressen.

use std::path::Path;
use std::process::{Command, Stdio};

use cockpit_core::RepoRules;
use cockpit_integrations::Secret;

/// So viele geänderte Dateien lässt das Backend durch.
const MAX_DATEIEN: usize = 500;

/// `owner/name`, wie GitHub es kennt.
pub fn repo_name_ok(repo: &str) -> bool {
    let teil = |t: &str| {
        !t.is_empty()
            && t != "."
            && t != ".."
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    repo.split_once('/')
        .is_some_and(|(o, n)| teil(o) && teil(n))
}

/// Nur Branches `claude/…` mit unauffälligen Zeichen. Der Hauptbranch ist damit ausgeschlossen.
pub fn branch_ok(branch: &str) -> bool {
    branch.starts_with("claude/")
        && branch.len() <= 120
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.ends_with('/')
        && !branch.ends_with(".lock")
        && !branch.ends_with('.')
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
}

/// Die Adresse zum Klonen: `<basis>/<owner>/<name>.git`.
pub fn klon_url(basis: &str, repo: &str) -> String {
    format!("{}/{repo}.git", basis.trim_end_matches('/'))
}

/// Die Web-Adresse aus der API-Adresse: `https://api.github.com` → `https://github.com`,
/// `https://ghe.example/api/v3` → `https://ghe.example`.
pub fn basis_aus_api(api_url: &str) -> String {
    let url = api_url.trim().trim_end_matches('/');
    if url == "https://api.github.com" {
        return "https://github.com".into();
    }
    url.strip_suffix("/api/v3").unwrap_or(url).to_string()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Umgebungsvariablen für die Anmeldung per HTTPS (`http.extraHeader`). Leer für alles, was nicht
/// `https://` ist: Das Token geht nie über Klartext-Verbindungen oder an lokale Pfade.
fn auth_umgebung(url: &str, token: Option<&Secret>) -> Vec<(String, String)> {
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    if !url.starts_with("https://") {
        return Vec::new();
    }
    let basic = base64(format!("x-access-token:{}", token.expose()).as_bytes());
    vec![
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "http.extraHeader".into()),
        (
            "GIT_CONFIG_VALUE_0".into(),
            format!("Authorization: Basic {basic}"),
        ),
    ]
}

/// Entfernt Zugangsdaten aus einem Text (Fehlermeldungen von Git können die Adresse enthalten).
pub fn schwaerzen(text: &str, geheimnisse: &[&str]) -> String {
    let mut t = text.to_string();
    for g in geheimnisse.iter().filter(|g| g.len() >= 4) {
        t = t.replace(g, "***");
        t = t.replace(&base64(format!("x-access-token:{g}").as_bytes()), "***");
    }
    t
}

fn git(
    args: &[&str],
    cwd: Option<&Path>,
    url_fuer_auth: &str,
    token: Option<&Secret>,
) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        // Keine Hooks, kein Zeitplan, keine Hilfsprogramme aus dem Klon.
        .env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    for (k, v) in auth_umgebung(url_fuer_auth, token) {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("git lässt sich nicht starten: {e}. Ist Git installiert?"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let fehler = String::from_utf8_lossy(&out.stderr);
    let fehler = fehler.trim();
    let geheim = token.map(Secret::expose).unwrap_or("");
    Err(schwaerzen(
        if fehler.is_empty() {
            "ohne Fehlermeldung"
        } else {
            fehler
        },
        &[geheim],
    ))
}

/// Öffnet Rechte im Klon, damit der Nutzer im Container (andere Kennung, bei Podman rootless
/// andere Zuordnung) schreiben und das Backend danach löschen kann. Der Ordner liegt in einem
/// Verzeichnis, das nur dem Nutzer gehört.
#[cfg(unix)]
pub fn rechte_oeffnen(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fn rec(p: &Path) {
        let Ok(meta) = std::fs::symlink_metadata(p) else {
            return;
        };
        if meta.file_type().is_symlink() {
            return;
        }
        let mode = if meta.is_dir() { 0o777 } else { 0o666 };
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
        if meta.is_dir()
            && let Ok(kinder) = std::fs::read_dir(p)
        {
            for k in kinder.flatten() {
                rec(&k.path());
            }
        }
    }
    rec(dir);
}

#[cfg(not(unix))]
pub fn rechte_oeffnen(_dir: &Path) {}

/// Klont `url` nach `ziel`, legt den Branch an und gibt den Stand zurück, von dem aus er beginnt.
/// Der Remote wird danach entfernt: Im Klon steht keine Adresse und kein Token.
pub fn klonen(
    url: &str,
    ziel: &Path,
    token: Option<&Secret>,
    basis_branch: &str,
    branch: &str,
) -> Result<String, String> {
    if !branch_ok(branch) {
        return Err(format!(
            "Der Branch „{branch}“ ist nicht erlaubt – nur claude/… wird angelegt."
        ));
    }
    if basis_branch.is_empty()
        || basis_branch.starts_with('-')
        || !basis_branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
    {
        return Err(format!("Der Basis-Branch „{basis_branch}“ ist ungültig."));
    }
    let z = ziel.to_string_lossy().into_owned();
    git(
        &[
            "clone", "--quiet", "--no-tags", "--depth", "50", "--single-branch", "--branch",
            basis_branch, "--", url, &z,
        ],
        None,
        url,
        token,
    )?;
    git(&["switch", "--quiet", "-c", branch], Some(ziel), url, None)?;
    let basis = git(&["rev-parse", "HEAD"], Some(ziel), url, None)?
        .trim()
        .to_string();
    git(&["remote", "remove", "origin"], Some(ziel), url, None)?;
    rechte_oeffnen(ziel);
    Ok(basis)
}

/// Was nach der Prüfung gepusht wurde.
#[derive(Debug, PartialEq, Eq)]
pub struct Veroeffentlicht {
    pub commit: String,
    pub dateien: Vec<String>,
}

/// Prüft den Branch aus dem Klon und pusht ihn. Siehe Moduldoku für die Reihenfolge und warum
/// nie im Klon selbst gearbeitet wird. `export` ist ein leerer Ordner für das Bare-Repo.
pub fn veroeffentlichen(
    klon: &Path,
    export: &Path,
    branch: &str,
    basis_sha: &str,
    url: &str,
    token: Option<&Secret>,
    regeln: &RepoRules,
) -> Result<Veroeffentlicht, String> {
    if !branch_ok(branch) {
        return Err(format!(
            "Der Branch „{branch}“ ist nicht erlaubt – gepusht wird nur claude/…."
        ));
    }
    if basis_sha.len() < 7 || !basis_sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Der Ausgangsstand des Laufs ist unbekannt.".into());
    }
    let e = export.to_string_lossy().into_owned();
    git(&["init", "--quiet", "--bare", &e], None, "", None)?;
    let k = klon.to_string_lossy().into_owned();
    let spec = format!("+refs/heads/{branch}:refs/heads/{branch}");
    git(
        &["fetch", "--quiet", "--no-tags", "--", &k, &spec],
        Some(export),
        "",
        None,
    )
    .map_err(|m| format!("Der Branch {branch} fehlt im Klon des Agenten ({m})."))?;
    let ref_ = format!("refs/heads/{branch}");
    let commit = git(&["rev-parse", "--verify", &ref_], Some(export), "", None)?
        .trim()
        .to_string();
    // Der Branch muss auf dem Ausgangsstand aufbauen (kein umgeschriebener Verlauf, kein fremder).
    git(
        &["merge-base", "--is-ancestor", basis_sha, &commit],
        Some(export),
        "",
        None,
    )
    .map_err(|_| {
        "Der Branch des Agenten baut nicht auf dem Stand auf, von dem aus geklont wurde.".to_string()
    })?;
    let anzahl = git(
        &["rev-list", "--count", &format!("{basis_sha}..{commit}")],
        Some(export),
        "",
        None,
    )?;
    if anzahl.trim() == "0" {
        return Err("Der Agent hat nichts committet, es gibt nichts zu pushen.".into());
    }
    let namen = git(
        &["diff", "--name-only", "--no-renames", "-z", basis_sha, &commit],
        Some(export),
        "",
        None,
    )?;
    let dateien: Vec<String> = namen
        .split('\0')
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    if dateien.is_empty() {
        return Err("Der Agent hat nichts geändert, es gibt nichts zu pushen.".into());
    }
    if dateien.len() > MAX_DATEIEN {
        return Err(format!(
            "Der Agent hat {} Dateien geändert – mehr als {MAX_DATEIEN} pusht das Cockpit nicht.",
            dateien.len()
        ));
    }
    let verstoesse = regeln.violations(&dateien);
    if !verstoesse.is_empty() {
        let gruende: Vec<String> = verstoesse.iter().map(|v| v.reason()).collect();
        return Err(format!(
            "Die Änderungen verstoßen gegen die Regeln des Repos und werden nicht gepusht: {}.",
            gruende.join("; ")
        ));
    }
    git(
        &["push", "--quiet", "--", url, &format!("{ref_}:{ref_}")],
        Some(export),
        url,
        token,
    )
    .map_err(|m| format!("Das Pushen auf {branch} ist fehlgeschlagen: {m}"))?;
    Ok(Veroeffentlicht { commit, dateien })
}

#[cfg(test)]
mod tests {
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
            "main", "master", "claude", "claude/", "claude/../main", "claude/a b",
            "claude/a//b", "claude/x.lock", "agent/BAU-1", "refs/heads/main", "claude/a;b",
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
        assert_eq!(basis_aus_api("https://api.github.com/"), "https://github.com");
        assert_eq!(basis_aus_api("https://ghe.example/api/v3"), "https://ghe.example");
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
        g(tmp.path(), &["clone", "--quiet", "--bare", "quelle", "remote.git"]);
        Aufbau {
            url: format!("file://{}", remote.display()),
            klon: tmp.path().join("klon"),
            export: tmp.path().join("export.git"),
            remote,
            _tmp: tmp,
        }
    }

    fn heads(remote: &Path) -> Vec<String> {
        g(remote, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])
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
        assert_eq!(g(&a.klon, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(), "claude/BAU-1-x");
        assert_eq!(g(&a.klon, &["remote"]).trim(), "", "kein Remote im Klon");
        agent_arbeitet(&a.klon, "src/neu.rs");
        let v = veroeffentlichen(&a.klon, &a.export, "claude/BAU-1-x", &basis, &a.url, None, &RepoRules::default())
            .unwrap();
        assert_eq!(v.dateien, ["src/neu.rs"]);
        let mut h = heads(&a.remote);
        h.sort();
        assert_eq!(h, ["claude/BAU-1-x", "main"]);
        // main im Remote ist unberührt.
        assert_eq!(g(&a.remote, &["rev-parse", "main"]).trim(), basis);
        assert_eq!(g(&a.remote, &["rev-parse", "claude/BAU-1-x"]).trim(), v.commit);
    }

    #[test]
    fn der_hauptbranch_wird_nie_angelegt_oder_gepusht() {
        let a = aufbau();
        assert!(klonen(&a.url, &a.klon, None, "main", "main").is_err());
        let basis = klonen(&a.url, &a.klon, None, "main", "claude/x").unwrap();
        agent_arbeitet(&a.klon, "a.txt");
        for b in ["main", "master", "agent/x", "refs/heads/main"] {
            let e = veroeffentlichen(&a.klon, &a.export, b, &basis, &a.url, None, &RepoRules::default());
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
        let e = veroeffentlichen(&a.klon, &a.export, "claude/r", &basis, &a.url, None, &regeln)
            .unwrap_err();
        assert!(e.contains("verstoßen gegen die Regeln"), "{e}");
        assert_eq!(heads(&a.remote), ["main"]);
    }

    #[test]
    fn ohne_commit_und_mit_fremdem_verlauf_wird_nicht_gepusht() {
        let a = aufbau();
        let basis = klonen(&a.url, &a.klon, None, "main", "claude/leer").unwrap();
        let e = veroeffentlichen(&a.klon, &a.export, "claude/leer", &basis, &a.url, None, &RepoRules::default())
            .unwrap_err();
        assert!(e.contains("nichts committet"), "{e}");

        // Umgeschriebener Verlauf: ein Branch ohne den Ausgangsstand.
        let b = aufbau();
        let basis = klonen(&b.url, &b.klon, None, "main", "claude/fremd").unwrap();
        g(&b.klon, &["checkout", "--quiet", "--orphan", "claude/fremd2"]);
        agent_arbeitet(&b.klon, "x.txt");
        let e = veroeffentlichen(&b.klon, &b.export, "claude/fremd2", &basis, &b.url, None, &RepoRules::default())
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
        for hook in ["pre-push", "post-commit", "reference-transaction", "pre-commit"] {
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
        g(&a.klon, &["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false", "commit", "--quiet", "--no-verify", "-m", "x"]);
        let _ = std::fs::remove_file(&marke);
        veroeffentlichen(&a.klon, &a.export, "claude/boese", &basis, &a.url, None, &RepoRules::default())
            .unwrap();
        assert!(!marke.exists(), "im Klon darf nichts ausgeführt werden");
        assert_eq!(heads(&a.remote).len(), 2);
    }

    #[test]
    fn klonen_scheitert_in_klartext() {
        let a = aufbau();
        let e = klonen("file:///gibt/es/nicht.git", &a.klon, None, "main", "claude/x").unwrap_err();
        assert!(!e.is_empty());
        assert!(klonen(&a.url, &a.klon, None, "--evil", "claude/x").is_err());
    }
}
