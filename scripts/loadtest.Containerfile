# The load generator (backend/examples/loadtest.rs) as a static musl binary,
# for scripts/loadtest.sh: it runs in the same podman network as the server
# under test, so the host's network stack (and its limit on ephemeral
# ports) is not part of the measurement.
FROM rust:1.99-slim AS build
WORKDIR /app/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/build.rs ./
COPY backend/migrations migrations/
COPY backend/src src/
COPY backend/examples examples/
# The crate embeds the frontend, which the example does not need: an empty
# folder satisfies rust-embed.
RUN mkdir -p /app/frontend/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/backend/target \
    target="$(uname -m)-unknown-linux-musl" \
    && apt-get update && apt-get install -y --no-install-recommends musl-tools \
    && rustup target add "$target" \
    && cargo build --release --example loadtest --target "$target" \
    && cp "target/$target/release/examples/loadtest" /loadtest

FROM alpine:3
COPY --from=build /loadtest /usr/local/bin/loadtest
ENTRYPOINT ["/usr/local/bin/loadtest"]
