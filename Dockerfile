# The image has three stages: the frontend build, the cargo build of the
# binary (which embeds the frontend), and the runtime, which is Alpine
# with just the static binary.
#
# Build:   mise run build:docker — for the architecture of the machine,
# natively, so an Apple Silicon Mac gets arm64 without emulation. The
# release builds amd64 and arm64 on native runners.

# The frontend: Vite builds it, and rust-embed embeds the result.
FROM node:22-bookworm-slim AS frontend
WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    corepack enable && pnpm install --frozen-lockfile
COPY frontend/ .
RUN pnpm build

# The compile: a single static binary (musl) with the frontend embedded.
# musl-tools provides the C toolchain for the C parts (sqlite, ring).
FROM rust:1.99-slim AS compile
WORKDIR /app/backend

# A profiling image is built with --build-arg PROFILE=profiling
# --build-arg FEATURES=profiling (mise run build:profiling); the default
# is the production build.
ARG PROFILE=release
ARG FEATURES=

# The target follows the build architecture, so one file builds amd64 and
# arm64 natively. rust-embed wants ../frontend/dist next to the crate. The
# cargo registry and target directory are cache mounts: a source change
# recompiles only the crate.
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/build.rs ./
COPY backend/migrations migrations/
COPY backend/src src/
COPY --from=frontend /app/dist /app/frontend/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/backend/target \
    target="$(uname -m)-unknown-linux-musl" \
    && apt-get update && apt-get install -y --no-install-recommends musl-tools \
    && rustup target add "$target" \
    && cargo build --profile "$PROFILE" --features "$FEATURES" --target "$target" \
    && cp "target/$target/$PROFILE/potoo" /potoo

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
