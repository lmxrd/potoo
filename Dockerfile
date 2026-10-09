# The image has three stages: the frontend build, the cross compile of the
# binary (which runs on the build machine's own platform, whatever the target
# is), and the runtime, which is Debian slim with just the binary.
#
# Build for the local platform:   deno task build:docker
# Build and push for both:        docker buildx build \
#                                   --platform linux/amd64,linux/arm64 \
#                                   -t <registry>/potoo --push .

# The frontend: Vite builds it, and the binary embeds the result.
FROM node:22-bookworm-slim AS frontend
WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY frontend/ .
RUN pnpm build

# The compile: QuickJS makes the binary smaller. The stage runs natively on
# the build platform and cross compiles with --target, so building the arm64
# image does not need an emulated compiler.
FROM --platform=$BUILDPLATFORM denoland/deno:2.9.7 AS compile
ARG TARGETARCH
WORKDIR /app
COPY deno.json deno.lock ./
COPY backend/ backend/
COPY --from=frontend /app/dist frontend/dist
RUN case "$TARGETARCH" in \
      arm64) TARGET=aarch64-unknown-linux-gnu ;; \
      *) TARGET=x86_64-unknown-linux-gnu ;; \
    esac \
    && deno compile -A --engine quickjs \
      --include backend/db/migrations --include frontend/dist \
      --target "$TARGET" --output /potoo backend/main.ts

# The runtime: Debian slim, the binary, and a writable /data for the
# database and the GeoIP database. bookworm-slim ships the CA certificates
# the GeoIP download needs.
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
