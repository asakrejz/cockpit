# Cockpit as a container image: the server and the built web app, nothing else.
#   docker build -f Containerfile -t cockpit:local .
# Not in the image: config/ (config.toml, cockpit.db, .env – a volume at /app/config), keys, the agent
# image (the app builds that itself, see docs/container.md). Builds with Docker, Podman and buildx.
#
# Multi-arch without emulation: the web and server stages always run on the build machine; the server is cross-compiled
# for $TARGETARCH. The web app (WebAssembly) is built once for both architectures.

# ---- web: web app (Trunk) ----------------------------------------------------------------------
# Does not depend on the target platform (WebAssembly), so BuildKit builds this stage once for both
# platforms. Its cache mounts are `sharing=locked` as a second guard against concurrent builds.
FROM --platform=$BUILDPLATFORM docker.io/library/rust:1-trixie AS web
ARG TRUNK_VERSION=0.22.2
WORKDIR /src
# rust-toolchain.toml first: `rustup target add` below must apply to the toolchain that builds the code.
COPY rust-toolchain.toml ./

RUN set -eu; \
    case "$(uname -m)" in \
      x86_64) trunk_arch=x86_64 ;; \
      aarch64) trunk_arch=aarch64 ;; \
      *) echo "unsupported build machine" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-${trunk_arch}-unknown-linux-musl.tar.gz" \
      | tar -xz -C /usr/local/bin trunk; \
    rustup target add wasm32-unknown-unknown

# wasm-bindgen in exactly the version of Cargo.lock. Trunk would download one, but its binaries may need a newer glibc than the image has.
COPY Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target-tools,sharing=locked \
    set -eu; \
    v="$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, "", $0); print; exit}' Cargo.lock)"; \
    CARGO_TARGET_DIR=/src/target-tools cargo install wasm-bindgen-cli --version "$v" --locked --root /usr/local

COPY Cargo.toml ./
COPY apps ./apps
COPY crates ./crates

# Web app -> apps/cockpit/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target-wasm,sharing=locked \
    cd apps/cockpit && CARGO_TARGET_DIR=/src/target-wasm trunk build --release

# ---- server: cross-compiled for $TARGETARCH on the build machine (no emulation) ------------------
FROM --platform=$BUILDPLATFORM docker.io/library/rust:1-trixie AS server
ARG TARGETARCH
# Per-platform cache id: the amd64 and arm64 builds run in parallel and must not share a target dir.
ARG TARGETPLATFORM
WORKDIR /src
COPY rust-toolchain.toml ./

# Cross compiler only if the target differs from the build machine.
RUN set -eu; \
    build="$(dpkg --print-architecture)"; \
    case "$TARGETARCH" in \
      arm64) triple=aarch64-unknown-linux-gnu; gnu=aarch64-linux-gnu; pkg=arm64 ;; \
      amd64) triple=x86_64-unknown-linux-gnu; gnu=x86-64-linux-gnu; pkg=amd64 ;; \
      *) echo "unsupported architecture $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$triple" > /triple; \
    if [ "$build" != "$TARGETARCH" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends "gcc-$gnu" "libc6-dev-$pkg-cross"; \
      rm -rf /var/lib/apt/lists/*; \
    fi; \
    rustup target add "$triple"

COPY Cargo.lock Cargo.toml ./
COPY apps ./apps
COPY crates ./crates

# Release (opt-level z + LTO from Cargo.toml), symbols stripped.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=target-native-$TARGETPLATFORM,target=/src/target-native,sharing=locked \
    set -eu; \
    triple="$(cat /triple)"; \
    export CARGO_TARGET_DIR=/src/target-native CARGO_PROFILE_RELEASE_STRIP=symbols; \
    case "$triple" in \
      aarch64-*) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc ;; \
      x86_64-*) [ "$(uname -m)" = x86_64 ] || export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc ;; \
    esac; \
    cargo build --release --locked -p cockpit-server --target "$triple"; \
    cp "/src/target-native/$triple/release/cockpit-server" /cockpit-server

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
COPY --from=server /cockpit-server /usr/local/bin/cockpit-server
# The working directory is the project folder: the server looks for config/, apps/cockpit/dist and apps/agent there.
COPY --from=web /src/apps/cockpit/dist /app/apps/cockpit/dist
# Containerfile and runner of the agent: "Build image" in the app needs them (the agent image itself is not part of this image).
COPY apps/agent /app/apps/agent

WORKDIR /app
USER cockpit

# Opt-in, only for the container: listen on all interfaces inside it. Publish the port on the host
# only as 127.0.0.1 (-p 127.0.0.1:8787:8787), see docs/security.md.
ENV COCKPIT_BIND=0.0.0.0 \
    COCKPIT_PORT=8787 \
    COCKPIT_CONTAINER=docker

VOLUME /app/config
EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=8s --start-period=10s --retries=3 \
    CMD ["cockpit-server", "health"]

ENTRYPOINT ["cockpit-server"]
