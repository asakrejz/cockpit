//! What the container runtime and the image look like right now (cached probes).

use cockpit_i18n::{Msg, msg};

use super::*;

impl AppState {
    pub(super) fn search_path(&self) -> OsString {
        self.inner
            .settings
            .search_path
            .clone()
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default()
    }

    /// The container runtime, remembered briefly. Blocking.
    pub(crate) fn container_status(&self) -> ContainerStatus {
        let e = &self.inner.settings;
        let path = self.search_path();
        self.inner
            .container
            .fetch(|| container::detect(e.container.as_deref(), e.container_env.as_deref(), &path))
    }

    pub(super) fn cli_path(&self, status: &ContainerStatus) -> Option<PathBuf> {
        container::in_path(status.runtime.as_deref()?, &self.search_path())
    }

    /// State of the agent image: missing, outdated, current or not checkable (`image inspect`
    /// with the fingerprint label against the fingerprint of the sources; remembered briefly).
    /// Blocking.
    pub(super) fn image_probe(&self, cli: &Path) -> ImageProbe {
        let mut c = lock(&self.inner.agent.image);
        if let Some((t, v)) = &*c
            && t.elapsed() < IMAGE_VALID
        {
            return v.clone();
        }
        let probe = self.image_probe_now(cli);
        *c = Some((Instant::now(), probe.clone()));
        probe
    }

    pub(super) fn image_probe_now(&self, cli: &Path) -> ImageProbe {
        let unknown = |hint: Msg| ImageProbe {
            state: ImageState::Unknown,
            hint: Some(hint),
        };
        let out = std::process::Command::new(cli)
            .args(agent_image::inspect_args())
            .arg(IMAGE)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                return unknown(msg!(ServerRunCliNotCallable, error = e.to_string()));
            }
        };
        // Docker and Podman both exit non-zero for an image that does not exist.
        if !out.status.success() {
            return ImageProbe {
                state: ImageState::Missing,
                hint: None,
            };
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let label = agent_image::label_value(&text);
        match agent_image::fingerprint(&self.agent_source()) {
            Ok(fp) if label == Some(fp.as_str()) => ImageProbe {
                state: ImageState::Current,
                hint: None,
            },
            Ok(_) => ImageProbe {
                state: ImageState::Outdated,
                hint: None,
            },
            Err(e) => unknown(e),
        }
    }

    pub(super) fn image_forget(&self) {
        *lock(&self.inner.agent.image) = None;
    }

    /// The key of the endpoint (`agent-key`).
    pub(super) fn agent_key(&self) -> Option<Secret> {
        match crate::app::agent_key_read(self.inner.secrets.as_ref()) {
            Ok(k) => k.filter(|s| !s.is_empty()),
            Err(e) => {
                tracing::warn!("Agent endpoint key not readable: {e}");
                None
            }
        }
    }
}
