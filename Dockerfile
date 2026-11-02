# Build with: docker build -t russh-bbs . (override the builder base with --build-arg RUST_IMAGE=...).
# Multi-arch (what the release workflow does): docker buildx build --platform linux/amd64,linux/arm64 .
# The Rust build always runs on the BUILD platform and cross-compiles for the target (D66), so an
# arm64 image never compiles under QEMU emulation and no RUN executes in the final stage.
# Behind a TLS-intercepting proxy the builder cannot reach rust-lang.org or crates.io; build the
# static musl binary on the host instead and package it unchanged (docs/deploy.md):
#   docker build --build-arg BIN_STAGE=prebuilt \
#     --build-context hostbin=target/x86_64-unknown-linux-musl/release -t russh-bbs .
# (arm64: add --platform linux/arm64 and use target/aarch64-unknown-linux-musl/release.)
# russh-bbs server image (D25): static musl binary on `scratch`, non-root, all state in /data.
# This is NOT the door sandbox: doors get their own isolation inside the process (D25).

ARG RUST_IMAGE=rust:1-slim
ARG PYTHON_IMAGE=python:3-slim
# Which stage supplies /out/russh-bbs-server: `build` (default) or `prebuilt` (host binary).
ARG BIN_STAGE=build

# --- zig: the C compiler and linker for the cross build (D66). `ring` and the bundled SQLite
# compile C and assembly for the target, so a bare Rust cross target is not enough; zig ships
# musl libc and a cross compiler for every target, and cargo-zigbuild wires it into cargo. Both
# come from PyPI wheels (the projects' own channel for a prebuilt zig), version- and hash-pinned
# for both build hosts (x86_64 and aarch64 Linux); pip refuses anything else.
FROM --platform=$BUILDPLATFORM ${PYTHON_IMAGE} AS zig
RUN --mount=type=cache,target=/root/.cache/pip <<'SH'
set -eu
cat > /tmp/zig-requirements.txt <<'REQ'
ziglang==0.17.0 \
    --hash=sha256:97079827064a7492cce5541ceb438ae8003ce1e54e399676da2d62fb7a5dc08b \
    --hash=sha256:d2804f94f54f7fcf8169f4b212a619ad688a4e2e6592e976f0cf2e58e2cd8af3
cargo-zigbuild==0.24.0 \
    --hash=sha256:484412e39e4c6039474535ce97ee75a22bab22d58233056f8ec8a8f9034bb957 \
    --hash=sha256:6942e4320f7b05d8fb29e0a8ce5670bcbb3d254275100b85b5b7c86552f9a744
REQ
pip install --no-deps --require-hashes --target /opt/zig -r /tmp/zig-requirements.txt
SH

# --- build: static musl binary, compiled on the build host for $TARGETARCH. rust-toolchain.toml
# pins the exact compiler version; the base tag only has to be new enough to host rustup, which
# then installs that toolchain plus the musl target std.
FROM --platform=$BUILDPLATFORM ${RUST_IMAGE} AS build
ARG TARGETARCH
COPY --from=zig /opt/zig /opt/zig
ENV PATH=/opt/zig/bin:/opt/zig/ziglang:$PATH
WORKDIR /src
COPY rust-toolchain.toml ./
RUN <<'SH'
set -eu
case "$TARGETARCH" in
  amd64) triple=x86_64-unknown-linux-musl ;;
  arm64) triple=aarch64-unknown-linux-musl ;;
  *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;;
esac
echo "$triple" > /triple
rustup target add "$triple"
SH
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# sqlx checks queries against the committed crates/store/.sqlx data, never a live database.
ENV SQLX_OFFLINE=true
# Static linking is the default for *-musl; zig compiles ring's and SQLite's C and asm for the target.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,id=russh-bbs-target-${TARGETARCH} <<'SH'
set -eu
triple=$(cat /triple)
cargo zigbuild --release --locked -p russh-bbs-server --target "$triple"
install -D "target/$triple/release/russh-bbs-server" /out/russh-bbs-server
install -d -m 0700 -o 65532 -g 65532 /out/data
SH

# --- alternative binary source: a binary built on the host (only used with BIN_STAGE=prebuilt).
# Runs on the build platform too: it only copies a file and makes a directory.
FROM --platform=$BUILDPLATFORM alpine:3 AS prebuilt
COPY --from=hostbin russh-bbs-server /out/russh-bbs-server
RUN install -d -m 0700 -o 65532 -g 65532 /out/data

# --- final: nothing but the binary and an empty, correctly-owned /data. No RUN here (and the
# bin stage is architecture-independent apart from the binary), so nothing needs emulation.
FROM ${BIN_STAGE} AS bin
FROM scratch
COPY --from=bin /out/russh-bbs-server /russh-bbs-server
# Created in the build stage so a fresh named volume inherits UID 65532 ownership.
COPY --from=bin --chown=65532:65532 /out/data /data
# Only the listen address is set. The image has no HOME, so the server's default data
# directory is /data (D46): host key /data/host_ed25519_key, SQLite /data/russh-bbs.db. Do not
# bake RUSSH_BBS_DB or RUSSH_BBS_HOST_KEY: a baked variable cannot be unset by `docker run`, and
# RUSSH_BBS_DB is a boot error with datasource.kind = "postgres" (QA M1).
ENV RUSSH_BBS_LISTEN=0.0.0.0:2222 \
    RUST_LOG=info
USER 65532:65532
VOLUME /data
EXPOSE 2222
ENTRYPOINT ["/russh-bbs-server"]
