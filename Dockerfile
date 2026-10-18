# The image has five stages: the cargo dependency recipe, the OpenAPI
# spec, the frontend build (which generates its API client from the
# spec), the cargo build of the binary (which embeds the frontend), and
# the runtime, which is Alpine with just the static binary.
#
# The cargo dependencies are built by cargo-chef into image layers, so
# the layer cache holds them: podman's or docker's locally, the GitHub
# Actions cache in CI (cache mounts, the alternative, are invisible to
# CI caches). A source change recompiles only the crate; a Cargo.lock
# change rebuilds the dependency layer.
#
# Build:   mise run build:docker — for the architecture of the machine,
# natively, so an Apple Silicon Mac gets arm64 without emulation. The
# release workflow builds amd64 and arm64 on native runners.

# The toolchain, with cargo-chef for the dependency layers.
FROM rust:1.99-slim AS chef
RUN cargo install --locked --version 0.1.78 cargo-chef

# The recipe: the minimum manifests of the dependency build. It re-runs
# on every source change, but its output changes only when the manifests
# do — that is what keeps the cook layers below cached.
FROM chef AS planner
WORKDIR /repo/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src src/
RUN cargo chef prepare --recipe-path /recipe.json

# The OpenAPI spec: the frontend's API client is generated from it, so
# the backend builds first, without the frontend. The `openapi` binary
# needs no database (src/bin/openapi.rs); it writes into target/, so the
# file is copied out to /spec. This stage cooks its own dependency layer
# (dev profile, host target); the binary below builds for musl.
FROM chef AS spec
WORKDIR /repo/backend
# No debugger ever runs in this stage; skipping debug info and incremental
# compilation cuts its memory use (and time) roughly in half.
ENV CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_INCREMENTAL=0
COPY --from=planner /recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json
COPY backend/build.rs build.rs
COPY backend/migrations migrations/
COPY backend/src src/
RUN cargo run --quiet --bin openapi \
    && mkdir /spec && cp target/specs/openapi.json /spec/

# The frontend: the client is generated from the spec (openapi-ts reads
# it at ../backend/target/specs/openapi.json, so this stage mirrors the
# repo layout), then Vite builds the app that rust-embed embeds next.
FROM node:26-slim AS frontend
WORKDIR /repo/frontend
# Node 26 ships without corepack, so the pnpm of frontend/package.json's
# packageManager field is installed explicitly.
RUN npm install --global pnpm@12.11.2
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/.pnpmfile.cjs ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile
COPY frontend/ .
COPY --from=spec /spec/openapi.json /repo/backend/target/specs/openapi.json
RUN pnpm run gen:api && pnpm run build

# The dependencies of the release binary (musl) as a cached layer: the
# heavy step of the build. The target follows the build architecture, so
# one file builds amd64 and arm64 natively.
FROM chef AS deps-musl
WORKDIR /repo/backend
RUN apt-get update \
    && apt-get install -y --no-install-recommends musl-tools \
    && rustup target add "$(uname -m)-unknown-linux-musl"
COPY --from=planner /recipe.json recipe.json
RUN cargo chef cook --release --target "$(uname -m)-unknown-linux-musl" --recipe-path recipe.json

# The compile: a single static binary (musl) with the frontend embedded;
# rust-embed wants ../frontend/dist next to the crate. A profiling image
# is built with --build-arg PROFILE=profiling --build-arg FEATURES=profiling
# (mise run build:profiling); the profiling profile compiles the
# dependency tree again — only the release layer above is cooked.
FROM deps-musl AS compile
ARG PROFILE=release
ARG FEATURES=
COPY backend/build.rs build.rs
COPY backend/migrations migrations/
COPY backend/src src/
COPY --from=frontend /repo/frontend/dist /repo/frontend/dist
RUN cargo build --profile "$PROFILE" --features "$FEATURES" --target "$(uname -m)-unknown-linux-musl" \
    && cp "target/$(uname -m)-unknown-linux-musl/$PROFILE/potoo" /potoo

# The runtime: Alpine, the static binary, and a writable /data for the
# database and the GeoIP database. ca-certificates is what the GeoIP
# download needs.
FROM alpine:3
RUN apk add --no-cache ca-certificates \
  && addgroup -S potoo && adduser -S -G potoo potoo \
  && mkdir /data && chown potoo /data
COPY --from=compile /potoo /usr/local/bin/potoo
ENV PORT=8000 \
    DATABASE_PATH=/data/potoo.db \
    GEOIP_DB_PATH=/data/country.mmdb
VOLUME /data
EXPOSE 8000

# The healthcheck: the backend answers /api/health with 200 OK. Shell
# form, so $PORT expands; wget is busybox's.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q -O /dev/null "http://127.0.0.1:$PORT/api/health" || exit 1

USER potoo
ENTRYPOINT ["/usr/local/bin/potoo"]
