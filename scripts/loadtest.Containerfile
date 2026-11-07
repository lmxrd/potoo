# The load generator (packages/backend/api/examples/loadtest.rs) as a static musl binary,
# for scripts/loadtest.sh: it runs in the same podman network as the server
# under test, so the host's network stack (and its limit on ephemeral
# ports) is not part of the measurement.
FROM rust:1.99-slim AS build
WORKDIR /app/backend
COPY packages/backend/Cargo.toml packages/backend/Cargo.lock ./
COPY packages/backend/src src/
COPY packages/backend/api api/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/backend/target \
    target="$(uname -m)-unknown-linux-musl" \
    && apt-get update && apt-get install -y --no-install-recommends musl-tools \
    && rustup target add "$target" \
    && cargo build --release --package api --example loadtest --target "$target" \
    && cp "target/$target/release/examples/loadtest" /loadtest

FROM alpine:3
COPY --from=build /loadtest /usr/local/bin/loadtest
ENTRYPOINT ["/usr/local/bin/loadtest"]
