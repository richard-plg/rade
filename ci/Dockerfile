# syntax=docker/dockerfile:1
# CI/release image for RADE (ADR-0012, ADR-0042). The runner needs only Docker
# (no Rust/Zig/GTK), so every build and test runs in here. Build context =
# the repo root (`docker build -f ci/Dockerfile .`); nothing is bind-mounted.
#
# Stages:  toolchain → src (libghostty) → check (fmt, clippy, unit tests incl. design tokens, contrast, AC trace, removed names)
#                                        → e2e   (release build; `docker run` the suites)
#                                          └→ deb (scripts/package-deb.sh; m10 runs here)
# Caches are BuildKit cache mounts (cargo registry/git, target dirs, zig
# caches): warm on a local daemon, cold on a fresh GitHub-hosted runner.

ARG UBUNTU=26.04
FROM ubuntu:${UBUNTU} AS toolchain
ARG RUST_VERSION=1.97.1
ARG ZIG_VERSION=0.16.0
ARG ZIG_SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    rm -f /etc/apt/apt.conf.d/docker-clean && apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl xz-utils git build-essential pkg-config gettext \
      libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev libgtksourceview-5-dev \
      libgl-dev libegl-dev libepoxy-dev libgl1-mesa-dri libegl-mesa0 mesa-libgallium \
      weston wl-clipboard dbus dbus-daemon libglib2.0-bin fuse3 \
      jq bc python3 python3-pil python3-gi gir1.2-gdkpixbuf-2.0 librsvg2-common \
      dpkg-dev binutils patchelf desktop-file-utils lintian ncurses-bin netcat-openbsd procps \
      fonts-dejavu-core fonts-noto-core \
      unzip libnss3 libnspr4 libgbm1 libasound2t64 libatk-bridge2.0-0t64 libcups2t64 \
      libxkbcommon0 libxcomposite1 libxdamage1 libxrandr2 libdrm2
# Chrome for Testing's headless shell for `rade browser --engine chromium`
# (M24, ADR-0026; tests/e2e/m24.sh finds it via RADE_E2E_CHROMIUM).
ARG CFT_VERSION=151.0.7922.34
ARG CFT_SHA256=3cfc2bd00d1bafcf8a68dc74c9c92bb7150ddc8d26ade948a776316e1cec4f14
RUN curl -fsSL -o /tmp/cft.zip "https://storage.googleapis.com/chrome-for-testing-public/${CFT_VERSION}/linux64/chrome-headless-shell-linux64.zip" \
    && echo "${CFT_SHA256}  /tmp/cft.zip" | sha256sum -c - \
    && unzip -q /tmp/cft.zip -d /opt && rm /tmp/cft.zip
ENV RADE_E2E_CHROMIUM=/opt/chrome-headless-shell-linux64/chrome-headless-shell
RUN curl -fsSL -o /tmp/zig.tar.xz "https://ziglang.org/download/${ZIG_VERSION}/zig-x86_64-linux-${ZIG_VERSION}.tar.xz" \
    && echo "${ZIG_SHA256}  /tmp/zig.tar.xz" | sha256sum -c - \
    && mkdir -p /opt/zig && tar -xJf /tmp/zig.tar.xz -C /opt/zig --strip-components=1 && rm /tmp/zig.tar.xz \
    && ln -s /opt/zig/zig /usr/local/bin/zig
# Non-root, like the owner's runs: some tests check permission errors.
RUN userdel -r ubuntu 2>/dev/null; useradd -m -u 1000 -s /bin/bash ci && install -d -o ci -g ci /src /cache
USER ci
ENV CARGO_HOME=/home/ci/.cargo RUSTUP_HOME=/home/ci/.rustup PATH=/home/ci/.cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" -c rustfmt,clippy
WORKDIR /src

# ── libghostty (zig) ─────────────────────────────────────────────────────────
FROM toolchain AS src
# The submodule alone first: libghostty (~9 min cold on a 4-vCPU runner) is a
# layer that only a submodule bump invalidates, so CI's layer cache (type=gha,
# ci.yml) skips it; the rest of the repo is copied after it.
COPY --chown=ci:ci ghostty /src/ghostty
# zig-pkg (fetched deps) and the caches are mounts; zig-out stays in the layer.
# Build with `docker build --network=host`: Zig's own DNS resolver fails
# (ResolvConfParseFailed) on the scoped IPv6 nameserver (`fe80::1%3`) Docker
# copies into build containers from this host's resolv.conf, and that file is
# read-only during RUN. The deb stage reuses the fetched zig-pkg.
RUN --mount=type=cache,target=/home/ci/.cache/zig,uid=1000,gid=1000 \
    --mount=type=cache,target=/src/ghostty/.zig-cache,uid=1000,gid=1000 \
    --mount=type=cache,target=/src/ghostty/zig-pkg,uid=1000,gid=1000 \
    # -Dcpu=baseline (ghostty/PACKAGING.md): Zig builds for the build
    # machine's CPU otherwise, and a layer cached on one runner then dies with
    # SIGILL on another, as would the .deb on an older CPU.
    cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast -Dcpu=baseline
COPY --chown=ci:ci . /src

# ── fmt + clippy + unit tests ────────────────────────────────────────────────
FROM src AS check
RUN --mount=type=cache,target=/home/ci/.cargo/registry,uid=1000,gid=1000 \
    --mount=type=cache,target=/home/ci/.cargo/git,uid=1000,gid=1000 \
    --mount=type=cache,target=/cache/target,uid=1000,gid=1000,sharing=locked \
    export CARGO_TARGET_DIR=/cache/target \
    && cargo fmt --all --check \
    && cargo clippy --workspace --all-targets --locked -- -D warnings \
    && cargo test --workspace --locked \
    && python3 docs/internals/design/contrast.py \
    && scripts/contrast_test.sh \
    && scripts/trace_test.sh \
    && scripts/trace.sh --no-write >/dev/null \
    && cargo build -p rade-cli --locked \
    && scripts/check-removed.sh --bin /cache/target/debug/rade

# ── release build for the E2E suites (run with `docker run`, see ci.yml) ─────
# Copied out of the cache mount into target/release; the CACHEDIR.TAG makes
# it the Devel flavor (ADR-0010) as in a local `cargo build --release`.
FROM src AS e2e
# m28's real-server smoke check (ADR-0029): rust-analyzer from the toolchain.
RUN rustup component add rust-analyzer
RUN --mount=type=cache,target=/home/ci/.cargo/registry,uid=1000,gid=1000 \
    --mount=type=cache,target=/home/ci/.cargo/git,uid=1000,gid=1000 \
    --mount=type=cache,target=/cache/target,uid=1000,gid=1000,sharing=locked \
    CARGO_TARGET_DIR=/cache/target cargo build --release --locked \
    && mkdir -p target/release \
    && echo 'Signature: 8a477f597d28d172789f06886806bc55' > target/CACHEDIR.TAG \
    && cp /cache/target/release/rade /cache/target/release/rade-app target/release/
# No GPU: Mesa llvmpipe. No user namespaces in the container for bwrap.
ENV NO_BUILD=1 LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1

# ── .deb ─────────────────────────────────────────────────────────────────────
# FROM e2e: tests/e2e/m10.sh also runs the dev build next to the package.
FROM e2e AS deb
ARG RADE_VERSION
ARG SOURCE_DATE_EPOCH
RUN --mount=type=cache,target=/home/ci/.cargo/registry,uid=1000,gid=1000 \
    --mount=type=cache,target=/home/ci/.cargo/git,uid=1000,gid=1000 \
    --mount=type=cache,target=/src/target/deb-build,uid=1000,gid=1000,sharing=locked \
    --mount=type=cache,target=/home/ci/.cache/zig,uid=1000,gid=1000 \
    --mount=type=cache,target=/src/ghostty/.zig-cache,uid=1000,gid=1000 \
    --mount=type=cache,target=/src/ghostty/zig-pkg,uid=1000,gid=1000 \
    RADE_VERSION="${RADE_VERSION:?build-arg RADE_VERSION}" SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:?build-arg SOURCE_DATE_EPOCH}" scripts/package-deb.sh
