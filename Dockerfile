# The image has three stages: the frontend build, the cargo build of the
# binary (which embeds the frontend), and the runtime, which is Alpine
# with just the static binary.
#
# Build:   mise run build:docker — an amd64 image, on any machine; on an
# arm64 host the whole build runs emulated (slow, but identical).

# The frontend: Vite builds it, and rust-embed embeds the result.
FROM node:22-bookworm-slim AS frontend
WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY frontend/ .
RUN pnpm build

# The compile: a single static binary (musl) with the frontend embedded.
# musl-tools provides the C toolchain for the C parts (sqlite, ring).
FROM rust:1-slim AS compile
WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src src/
COPY --from=frontend /app/dist frontend/dist
RUN rustup target add x86_64-unknown-linux-musl \
    && apt-get update && apt-get install -y --no-install-recommends musl-tools \
    && rm -rf /var/lib/apt/lists/* \
    && cargo build --release --target x86_64-unknown-linux-musl \
    && cp target/x86_64-unknown-linux-musl/release/potoo /potoo

# The runtime: Alpine, the static binary, and a writable /data for the
# database and the GeoIP database. ca-certificates is what the GeoIP
# download needs.
FROM alpine:3
RUN addgroup -S potoo && adduser -S -G potoo potoo \
  && mkdir /data && chown potoo /data
COPY --from=compile /potoo /usr/local/bin/potoo
RUN apk add --no-cache ca-certificates
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
