# Cockpit as a container image: the server and the built web app, nothing else.
#   docker build -f Containerfile -t cockpit:local .
# Not in the image: config/ (config.toml, cockpit.db, .env – a volume at /app/config), keys, the agent
# image (the app builds that itself, see docs/container.md). Builds with Docker, Podman and buildx.
#
# Multi-arch without emulation: the web and server stages always run on the build machine; the server is cross-compiled
# for $TARGETARCH. The web app (WebAssembly) is built once for both architectures.
#
# Layer cache: the dependencies of the Rust code are compiled in their own layer (cargo-chef), which only changes when
# Cargo.toml or Cargo.lock change. A change to the source then only recompiles the workspace crates. Everything lives in
# image layers, not in BuildKit cache mounts, because only layers are exported to a CI cache (`type=gha`); the per-platform
# target dirs are separate layers, so the amd64 and arm64 builds that run in parallel never share one.

# ---- chef: Rust toolchain + cargo-chef (prebuilt, no compile) --------------------------------------
FROM --platform=$BUILDPLATFORM docker.io/library/rust:1-trixie AS chef
ARG CHEF_VERSION=0.1.78
WORKDIR /src
# rust-toolchain.toml first: `rustup target add` below must apply to the toolchain that builds the code.
COPY rust-toolchain.toml ./
RUN set -eu; \
    case "$(uname -m)" in \
      x86_64) arch=x86_64 ;; \
      aarch64) arch=aarch64 ;; \
      *) echo "unsupported build machine" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/LukeMathWalker/cargo-chef/releases/download/v${CHEF_VERSION}/cargo-chef-${arch}-unknown-linux-musl.tar.xz" \
      | tar -xJ -C /usr/local/bin --strip-components=1 --wildcards '*/cargo-chef'; \
    cargo chef --version

# ---- planner: what the dependencies are ------------------------------------------------------------
# Runs on every source change but only reads the manifests: its two outputs (the recipe and the wasm-bindgen version)
# are the same for the same dependencies, and the stages below copy exactly these two files, so they keep their cache.
# apps/agent is not copied: it is Node code that no Rust build needs.
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY apps/cockpit ./apps/cockpit
COPY apps/playground ./apps/playground
COPY apps/server ./apps/server
COPY crates ./crates
RUN set -eu; \
    cargo chef prepare --recipe-path /recipe.json; \
    awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, "", $0); print; exit}' Cargo.lock > /wasm-bindgen-version; \
    test -s /wasm-bindgen-version

# ---- web: web app (Trunk) ----------------------------------------------------------------------
# Does not depend on the target platform (WebAssembly), so BuildKit builds this stage once for both platforms.
FROM chef AS web
ARG TRUNK_VERSION=0.22.2
RUN set -eu; \
    case "$(uname -m)" in \
      x86_64) trunk_arch=x86_64 ;; \
      aarch64) trunk_arch=aarch64 ;; \
      *) echo "unsupported build machine" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-${trunk_arch}-unknown-linux-musl.tar.gz" \
      | tar -xz -C /usr/local/bin trunk; \
    rustup target add wasm32-unknown-unknown

# wasm-bindgen in exactly the version of Cargo.lock (static musl binary from its release; Trunk would download one itself
# at every build). Only the version file is copied, so this layer survives any other change to Cargo.lock. If the release
# has no such binary, it is compiled instead.
COPY --from=planner /wasm-bindgen-version /wasm-bindgen-version
RUN set -eu; \
    v="$(cat /wasm-bindgen-version)"; \
    case "$(uname -m)" in x86_64) arch=x86_64 ;; aarch64) arch=aarch64 ;; esac; \
    if curl -fsSL "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${v}/wasm-bindgen-${v}-${arch}-unknown-linux-musl.tar.gz" \
        | tar -xz -C /usr/local/bin --strip-components=1 --wildcards '*/wasm-bindgen'; then \
      :; \
    else \
      CARGO_TARGET_DIR=/tmp/target-tools cargo install wasm-bindgen-cli --version "$v" --locked --root /usr/local; \
      rm -rf /tmp/target-tools /usr/local/cargo/registry; \
    fi; \
    wasm-bindgen --version | grep -F "$v"

ENV CARGO_TARGET_DIR=/src/target-wasm
# The dependencies, compiled once. Cached until a Cargo.toml or the Cargo.lock changes.
COPY --from=planner /recipe.json recipe.json
RUN cargo chef cook --release --locked --target wasm32-unknown-unknown --package cockpit \
      --recipe-path recipe.json --target-dir "$CARGO_TARGET_DIR"

COPY Cargo.toml Cargo.lock ./
COPY apps/cockpit ./apps/cockpit
COPY apps/playground ./apps/playground
COPY apps/server ./apps/server
COPY crates ./crates

# Web app -> apps/cockpit/dist. `touch`: the sources must count as newer than the placeholder build of the cook step above.
RUN find apps crates -name '*.rs' -exec touch {} + \
    && cd apps/cockpit && trunk build --release

# ---- server: cross-compiled for $TARGETARCH on the build machine (no emulation) ------------------
FROM chef AS server
ARG TARGETARCH

# Cross compiler only if the target differs from the build machine. /cross-env.sh holds the linker settings for it.
RUN set -eu; \
    build="$(dpkg --print-architecture)"; \
    case "$TARGETARCH" in \
      arm64) triple=aarch64-unknown-linux-gnu; gnu=aarch64-linux-gnu; pkg=arm64 ;; \
      amd64) triple=x86_64-unknown-linux-gnu; gnu=x86-64-linux-gnu; pkg=amd64 ;; \
      *) echo "unsupported architecture $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$triple" > /triple; \
    : > /cross-env.sh; \
    if [ "$build" != "$TARGETARCH" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends "gcc-$gnu" "libc6-dev-$pkg-cross"; \
      rm -rf /var/lib/apt/lists/*; \
      case "$triple" in \
        aarch64-*) echo 'export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc' > /cross-env.sh ;; \
        x86_64-*) echo 'export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc' > /cross-env.sh ;; \
      esac; \
    fi; \
    rustup target add "$triple"

# Release (opt-level z + LTO from Cargo.toml), symbols stripped. The same settings for the dependencies and the final build.
ENV CARGO_TARGET_DIR=/src/target-native \
    CARGO_PROFILE_RELEASE_STRIP=symbols
# The dependencies, compiled once per platform. Cached until a Cargo.toml or the Cargo.lock changes.
COPY --from=planner /recipe.json recipe.json
RUN set -eu; \
    . /cross-env.sh; \
    cargo chef cook --release --locked --target "$(cat /triple)" --package cockpit-server \
      --recipe-path recipe.json --target-dir "$CARGO_TARGET_DIR"

COPY Cargo.lock Cargo.toml ./
COPY apps/cockpit ./apps/cockpit
COPY apps/playground ./apps/playground
COPY apps/server ./apps/server
COPY crates ./crates

# `touch` as in the web stage above.
RUN set -eu; \
    . /cross-env.sh; \
    triple="$(cat /triple)"; \
    find apps crates -name '*.rs' -exec touch {} +; \
    cargo build --release --locked -p cockpit-server --target "$triple"; \
    cp "$CARGO_TARGET_DIR/$triple/release/cockpit-server" /cockpit-server

# ---- docker CLI (static binary, only used if the host socket is mounted) ------------------
FROM docker.io/library/docker:29-cli AS dockercli

# ---- final image ------------------------------------------------------------------------------
FROM docker.io/library/debian:trixie-slim

# git: the backend clones the repos of the agent with it. ca-certificates: HTTPS to Jira, GitHub and the model endpoint.
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --user-group --no-create-home --shell /usr/sbin/nologin cockpit \
    && mkdir -p /app/config \
    && chown -R cockpit:cockpit /app

COPY --from=dockercli /usr/local/bin/docker /usr/local/bin/docker
# The real binary lives outside the PATH. `cockpit-server` (for `docker exec`, the healthcheck) and the entrypoint are
# small scripts that drop root first: the server never runs as root. See container/entrypoint.sh.
COPY --from=server /cockpit-server /usr/local/libexec/cockpit-server
COPY --chmod=0755 container/cockpit-server.sh /usr/local/bin/cockpit-server
COPY --chmod=0755 container/entrypoint.sh /usr/local/bin/cockpit-entrypoint
# The working directory is the project folder: the server looks for config/, apps/cockpit/dist and apps/agent there.
COPY --from=web /src/apps/cockpit/dist /app/apps/cockpit/dist
# Containerfile and runner of the agent: "Build image" in the app needs them (the agent image itself is not part of this image).
COPY apps/agent /app/apps/agent

WORKDIR /app
# No `USER`: the entrypoint starts as root only to pick up the group of a mounted engine socket and then drops to
# `cockpit` (uid 10001) with setpriv. Without a socket it simply drops.

# Opt-in, only for the container: listen on all interfaces inside it. Publish the port on the host
# only as 127.0.0.1 (-p 127.0.0.1:8787:8787), see docs/security.md.
ENV COCKPIT_BIND=0.0.0.0 \
    COCKPIT_PORT=8787 \
    COCKPIT_CONTAINER=docker \
    COCKPIT_IN_CONTAINER=1

VOLUME /app/config
EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=8s --start-period=10s --retries=3 \
    CMD ["cockpit-server", "health"]

ENTRYPOINT ["cockpit-entrypoint"]
