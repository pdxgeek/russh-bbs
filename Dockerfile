# Build with: docker build -t russh-bbs . (override the builder base with --build-arg RUST_IMAGE=...).
# russh-bbs server image (D25): static musl binary on `scratch`, non-root, all state in /data.
# This is NOT the door sandbox: doors get their own isolation inside the process (D25).

# --- build: static musl binary. rust-toolchain.toml pins the exact compiler version; the
# base tag only has to be new enough to host rustup, which then installs that toolchain.
ARG RUST_IMAGE=rust:1-alpine
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

# --- final: nothing but the binary and an empty, correctly-owned /data.
FROM scratch
COPY --from=build /out/russh-bbs-server /russh-bbs-server
# Created in the build stage so a fresh named volume inherits UID 65532 ownership.
COPY --from=build --chown=65532:65532 /out/data /data
ENV RUSSH_BBS_LISTEN=0.0.0.0:2222 \
    RUSSH_BBS_HOST_KEY=/data/host_ed25519_key \
    RUSSH_BBS_DB=/data/russh-bbs.db \
    RUST_LOG=info
USER 65532:65532
VOLUME /data
EXPOSE 2222
ENTRYPOINT ["/russh-bbs-server"]
