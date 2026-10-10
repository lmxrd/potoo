# The image has three stages: the frontend build, the cargo build of the
# binary (which embeds the frontend), and the runtime, which is Debian
# slim with just the binary.
#
# Build for the local platform:   mise run build:docker
# Build and push for both:        docker buildx build \
#                                   --platform linux/amd64,linux/arm64 \
#                                   -t <registry>/potoo --push .
#
# The compile stage runs on the build platform's own architecture and
# cross compiles with --target, so building the arm64 image does not
# need an emulated compiler.

# The frontend: Vite builds it, and rust-embed embeds the result.
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS frontend
WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY frontend/ .
RUN pnpm build

# The compile: a single static binary with the frontend embedded.
FROM --platform=$BUILDPLATFORM rust:1-slim AS compile
ARG TARGETARCH
WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src src/
COPY --from=frontend /app/dist frontend/dist
RUN case "$TARGETARCH" in \
      arm64) \
        rustup target add aarch64-unknown-linux-gnu \
        && apt-get update && apt-get install -y --no-install-recommends \
             gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
        && rm -rf /var/lib/apt/lists/* \
        && cargo build --release --target aarch64-unknown-linux-gnu \
        && cp target/aarch64-unknown-linux-gnu/release/potoo /potoo \
      ;; \
      *) cargo build --release && cp target/release/potoo /potoo ;; \
    esac

# The runtime: Debian slim, the binary, and a writable /data for the
# database and the GeoIP database. bookworm-slim ships the CA
# certificates the GeoIP download needs.
FROM debian:bookworm-slim
RUN useradd --system --create-home potoo \
  && mkdir /data && chown potoo /data
COPY --from=compile /potoo /usr/local/bin/potoo
ENV PORT=8000 \
    DATABASE_PATH=/data/potoo.db \
    GEOIP_DB_PATH=/data/country.mmdb
VOLUME /data
EXPOSE 8000
USER potoo
ENTRYPOINT ["/usr/local/bin/potoo"]
