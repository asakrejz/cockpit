use super::*;

fn all_ok(_: &str) -> bool {
    true
}
fn no(_: &str) -> bool {
    false
}

#[test]
fn without_choice_comes_podman_before_docker() {
    let s = choose(None, None, all_ok, all_ok);
    assert_eq!(s.runtime.as_deref(), Some("podman"));
    assert!(s.engine_running && s.hint.is_none());
    assert_eq!(s.source, "detected");
}

#[test]
fn running_podman_not_is_docker_taken() {
    let s = choose(None, None, all_ok, |cli| cli == "docker");
    assert_eq!(s.runtime.as_deref(), Some("docker"));
    assert!(s.engine_running);
    let s = choose(None, None, |cli| cli == "docker", all_ok);
    assert_eq!(s.runtime.as_deref(), Some("docker"));
}

#[test]
fn present_but_without_engine_gives_a_hint() {
    let s = choose(None, None, all_ok, no);
    assert_eq!(s.runtime.as_deref(), Some("podman"), "die erste gefundene");
    assert!(!s.engine_running);
    assert!(s.hint.unwrap().contains("podman machine start"));
    let s = choose(None, None, |c| c == "docker", no);
    assert!(s.hint.unwrap().contains("dockerd (moby)"));
}

#[test]
fn without_both_gives_it_no_runtime() {
    let s = choose(None, None, no, no);
    assert_eq!(s.runtime, None);
    assert_eq!(s.source, "none");
    assert!(s.hint.unwrap().contains("Weder podman noch docker"));
}

#[test]
fn explicit_choice_goes_before_and_env_before_file() {
    let s = choose(Some("podman"), Some("docker"), all_ok, all_ok);
    assert_eq!(s.runtime.as_deref(), Some("docker"));
    assert_eq!(s.source, "env");
    let s = choose(Some("docker"), None, all_ok, all_ok);
    assert_eq!(s.runtime.as_deref(), Some("docker"));
    assert_eq!(s.source, "config");
    // Chosen means chosen: it does not fall back to the other runtime.
    let s = choose(Some("docker"), None, all_ok, |cli| cli == "podman");
    assert_eq!(s.runtime.as_deref(), Some("docker"));
    assert!(!s.engine_running);
    let s = choose(Some("docker"), None, |cli| cli == "podman", all_ok);
    assert!(s.hint.unwrap().contains("nicht im PATH"));
}

#[cfg(unix)]
mod process {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn program(dir: &std::path::Path, name: &str, script: &str) {
        let file = dir.join(name);
        std::fs::write(&file, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn calls_info_on_and_evaluates_the_status() {
        let dir = tempfile::tempdir().unwrap();
        program(dir.path(), "podman", "[ \"$1\" = info ] && exit 1; exit 2");
        program(dir.path(), "docker", "[ \"$1\" = info ] && exit 0; exit 2");
        let s = detect(None, None, dir.path().as_os_str());
        assert_eq!(s.runtime.as_deref(), Some("docker"));
        assert!(s.engine_running);
        let s = detect(Some("podman"), None, dir.path().as_os_str());
        assert_eq!(s.runtime.as_deref(), Some("podman"));
        assert!(!s.engine_running);
    }

    #[test]
    fn a_file_without_exec_permission_counts_not() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docker"), "#!/bin/sh\nexit 0\n").unwrap();
        let s = detect(None, None, dir.path().as_os_str());
        assert_eq!(s.runtime, None);
    }
}

#[test]
fn cache_asks_only_once() {
    let store = Cache::default();
    let mut counter = 0;
    for _ in 0..3 {
        store.fetch(|| {
            counter += 1;
            choose(None, None, no, no)
        });
    }
    assert_eq!(counter, 1);
}
