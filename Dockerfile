# The image has three stages: the frontend build, the cargo build of the
# binary (which embeds the frontend), and the runtime, which is Alpine
# with just the static binary.
#
# Build for the local platform:   mise run build:docker
# Build and push for both:        docker buildx build \
#                                   --platform linux/amd64,linux/arm64 \
#                                   -t <registry>/potoo --push .
#
# The runtime can be Alpine because the binary targets musl and is
# fully static. The compile stage runs on the build platform's own
# architecture and cross compiles with --target, so building the arm64
# image does not need an emulated compiler.

# The frontend: Vite builds it, and rust-embed embeds the result.
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS frontend
WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY frontend/ .
RUN pnpm build

# The compile: a single static binary (musl) with the frontend embedded.
# The native build needs musl-tools for the C parts (sqlite, ring); the
# arm64 cross build uses the musl.cc toolchain, since Debian has no
# aarch64 musl cross compiler.
FROM --platform=$BUILDPLATFORM rust:1-slim AS compile
ARG TARGETARCH
WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src src/
COPY --from=frontend /app/dist frontend/dist
RUN case "$TARGETARCH" in \
      arm64) \
        apt-get update && apt-get install -y --no-install-recommends curl \
        && rm -rf /var/lib/apt/lists/* \
        && rustup target add aarch64-unknown-linux-musl \
        && curl -fsSL https://musl.cc/aarch64-linux-musl-cross.tgz \
             | tar -xz -C /usr/local \
        && export PATH="/usr/local/aarch64-linux-musl-cross/bin:$PATH" \
        && export CC_aarch64_unknown_linux_musl=aarch64-linux-musl-gcc \
               AR_aarch64_unknown_linux_musl=aarch64-linux-musl-ar \
               CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc \
        && cargo build --release --target aarch64-unknown-linux-musl \
        && cp target/aarch64-unknown-linux-musl/release/potoo /potoo \
      ;; \
      *) \
        rustup target add x86_64-unknown-linux-musl \
        && apt-get update && apt-get install -y --no-install-recommends musl-tools \
        && rm -rf /var/lib/apt/lists/* \
        && cargo build --release --target x86_64-unknown-linux-musl \
        && cp target/x86_64-unknown-linux-musl/release/potoo /potoo \
      ;; \
    esac

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
