# Build with: docker build -t russh-bbs . (override the builder base with --build-arg RUST_IMAGE=...).
# Behind a TLS-intercepting proxy the builder cannot reach rust-lang.org or crates.io; build the
# static musl binary on the host instead and package it unchanged (docs/deploy.md):
#   docker build --build-arg BIN_STAGE=prebuilt \
#     --build-context hostbin=target/x86_64-unknown-linux-musl/release -t russh-bbs .
# russh-bbs server image (D25): static musl binary on `scratch`, non-root, all state in /data,
# plus the build stage's CA bundle so https feeds can be verified (D91).
# This is NOT the door sandbox: doors get their own isolation inside the process (D25).

# --- build: static musl binary. rust-toolchain.toml pins the exact compiler version; the
# base tag only has to be new enough to host rustup, which then installs that toolchain.
ARG RUST_IMAGE=rust:1-alpine
# Which stage supplies /out/russh-bbs-server: `build` (default) or `prebuilt` (host binary).
ARG BIN_STAGE=build
FROM ${RUST_IMAGE} AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates ./crates
# sqlx checks queries against the committed crates/store/.sqlx data, never a live database.
ENV SQLX_OFFLINE=true
# Static linking is the default for *-musl; ring builds its C parts with the musl C compiler.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p russh-bbs-server \
    && install -D target/release/russh-bbs-server /out/russh-bbs-server \
    && install -d -m 0700 -o 65532 -g 65532 /out/data

# --- alternative binary source: a binary built on the host (only used with BIN_STAGE=prebuilt).
FROM alpine:3 AS prebuilt
COPY --from=hostbin russh-bbs-server /out/russh-bbs-server
RUN install -d -m 0700 -o 65532 -g 65532 /out/data

# --- final: the binary, the CA bundle and an empty, correctly-owned /data. Nothing else.
FROM ${BIN_STAGE} AS bin
FROM scratch
COPY --from=bin /out/russh-bbs-server /russh-bbs-server
# Alpine's Mozilla CA bundle (both `build` and `prebuilt` stages carry it). Without it no https
# feed can be verified (QA M3 M3). A sysop can replace it for feeds with `feeds.ca_file`.
COPY --from=bin /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
# Created in the build stage so a fresh named volume inherits UID 65532 ownership.
COPY --from=bin --chown=65532:65532 /out/data /data
# Only the listen address is set. The image has no HOME, so the server's default data
# directory is /data (D46): host key /data/host_ed25519_key, SQLite /data/russh-bbs.db. Do not
# bake RUSSH_BBS_DB or RUSSH_BBS_HOST_KEY: a baked variable cannot be unset by `docker run`, and
# RUSSH_BBS_DB is a boot error with datasource.kind = "postgres" (QA M1).
ENV RUSSH_BBS_LISTEN=0.0.0.0:2222 \
    RUST_LOG=info \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
USER 65532:65532
VOLUME /data
EXPOSE 2222
ENTRYPOINT ["/russh-bbs-server"]
