# The image is the runtime only: Alpine and the static (musl) binary,
# which is built beforehand, outside the image, so that the build uses the
# machine's own cargo cache rather than a cold one in a container:
#
#   mise run build:docker   (builds dist/potoo-linux-<arch>, then this)
#
# The binary embeds the frontend and the database schema. TARGETARCH
# (amd64 or arm64) is set by podman and buildx for the platform built for;
# `mise run build:linux` with ARCH=amd64 or arm64 builds either one, on a
# Mac too. A profiling image passes its own binary with --build-arg BINARY
# (mise run build:profiling).
FROM alpine:3
ARG TARGETARCH
ARG BINARY=dist/potoo-linux-${TARGETARCH}

# A writable /data for the database and the GeoIP database.
# ca-certificates is what the GeoIP download needs.
RUN apk add --no-cache ca-certificates \
  && addgroup -S potoo && adduser -S -G potoo potoo \
  && mkdir /data && chown potoo /data
COPY ${BINARY} /usr/local/bin/potoo
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
