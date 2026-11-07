# The image only packages a prebuilt, static (musl) binary — nothing
# compiles here. The binary comes from `mise run build:linux`
# (scripts/build-linux.sh, what CI and the release workflow run too), which
# writes dist/potoo-linux-<arch>:
#
#   mise run build:docker
#   docker build .                       # BuildKit, buildx and podman set TARGETARCH
#   docker build --build-arg BINARY=dist/potoo-linux-arm64-dev .
#   mise run build:profiling             # a CPU-profiling image (never push it)
#
# The runtime is Alpine, for a shell (`docker exec -it <container> sh`)
# and busybox tools when debugging in production. The binary itself is
# static and needs nothing from the base.
# An exact Alpine release, so the base changes only in a pull request
# (Dependabot's, as fix(deps): a release follows). `apk upgrade` takes the
# package fixes Alpine published since that release.
FROM alpine:3.24.4
ARG TARGETARCH
ARG BINARY=dist/potoo-linux-${TARGETARCH}
RUN apk upgrade --no-cache \
  && apk add --no-cache ca-certificates \
  && addgroup -S potoo && adduser -S -G potoo potoo \
  && mkdir /data && chown potoo /data
# --chmod: the executable bit does not survive every way a binary reaches dist/.
COPY --chmod=755 ${BINARY} /usr/local/bin/potoo
ENV PORT=8000 \
    DATABASE_URL=sqlite:/data/potoo.db \
    GEOIP_DB_PATH=/data/country.mmdb
VOLUME /data
EXPOSE 8000

# `potoo healthcheck` asks the running server's /api/health.
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/potoo", "healthcheck"]

USER potoo
ENTRYPOINT ["/usr/local/bin/potoo"]
