#!/usr/bin/env bash
# Write-load test of the tracker against the production image, in a
# container limited like a small server. The generator (backend/examples/
# loadtest.rs) runs in a container on the same network, so the host's
# network stack is not measured, and sends POST /api/collect at each rate in
# turn; per step this prints throughput, latency and the container's CPU
# and memory, read from its cgroup.
#
#   mise run loadtest                         # 2 CPUs, 2 GB, the default ramp
#   CPUS=1 RATES=2000,5000,8000 mise run loadtest
#
# TARGET=goatcounter runs the same ramp against GoatCounter's official image
# (hits as `GET /count`, its rate limit raised, its default 10 s write
# buffer left on), for a head-to-head under the same container limits.
#
# Settings (environment): TARGET (potoo), CPUS (2), MEMORY (2g), RATES (a comma separated
# list of requests per second), DURATION (seconds per step, 15), SYNCHRONOUS
# (SQLITE_SYNCHRONOUS for the server), IMAGE (skip the build and test this
# image, e.g. potoo:profiling from `mise run build:profiling`, whose phase
# timers are printed at the end), PORT (18000).
set -euo pipefail

TARGET=${TARGET:-potoo}
CPUS=${CPUS:-2}
MEMORY=${MEMORY:-2g}
RATES=${RATES:-500,1000,2000,5000,8000,12000,16000,20000}
SECONDS_PER_STEP=${DURATION:-15}
SYNCHRONOUS=${SYNCHRONOUS:-NORMAL}
PORT=${PORT:-18000}
NAME=potoo-loadtest
cd "$(dirname "$0")/.."

IMAGE=${IMAGE:-}
if [ "$TARGET" = goatcounter ]; then
  IMAGE=${IMAGE:-docker.io/arp242/goatcounter:latest}
elif [ -z "$IMAGE" ]; then
  IMAGE=potoo:loadtest
  echo "building $IMAGE"
  podman build -q -t "$IMAGE" . >/dev/null
fi
echo "building the load generator"
podman build -q -f scripts/loadtest.Containerfile -t potoo:loadgen . >/dev/null
NET=$NAME

jar=$(mktemp)
cleanup() {
  podman rm -f "$NAME" >/dev/null 2>&1 || true
  podman volume rm -f "$NAME" >/dev/null 2>&1 || true
  podman network rm -f "$NET" >/dev/null 2>&1 || true
  rm -f "$jar"
}
trap cleanup EXIT
cleanup

# A fresh database each run, on a podman volume (the VM's own disk, not a
# slow host mount). TRUST_PROXY lets the generator fake client addresses.
podman volume create "$NAME" >/dev/null
podman network create "$NET" >/dev/null
if [ "$TARGET" = goatcounter ]; then
  HOST=gc.example.com
  DATA=/home/goatcounter/goatcounter-data
  DB="sqlite+$DATA/gc.sqlite3"
  URL="http://$NAME:8080"
  MODE_ARGS=(--mode goatcounter --host "$HOST")
  podman run --rm -v "$NAME:$DATA" "$IMAGE" db create site -createdb -vhost "$HOST" \
    -user.email load@example.com -user.password load-test-password-1 -db "$DB" >/dev/null 2>&1
  podman run -d --name "$NAME" --memory="$MEMORY" --memory-swap="$MEMORY" --cpus="$CPUS" --network "$NET" \
    -p "$PORT:8080" -v "$NAME:$DATA" "$IMAGE" serve -listen :8080 -tls none -automigrate \
    -db "$DB" -ratelimit count:100000000/1 >/dev/null
  until curl -s -o /dev/null -H "Host: $HOST" "localhost:$PORT/count?p=%2F"; do sleep 0.5; done
else
  URL="http://$NAME:8000"
  MODE_ARGS=()
  podman run -d --name "$NAME" --memory="$MEMORY" --memory-swap="$MEMORY" --cpus="$CPUS" --network "$NET" \
    -p "$PORT:8000" -v "$NAME:/data" -e TRUST_PROXY=1 -e SQLITE_SYNCHRONOUS="$SYNCHRONOUS" \
    "$IMAGE" >/dev/null
  until curl -sf "localhost:$PORT/api/health" >/dev/null; do sleep 0.5; done
  curl -sf -c "$jar" -X POST "localhost:$PORT/api/setup" \
    -d '{"email":"load@example.com","password":"load-test-password-1"}' >/dev/null
  curl -sf -b "$jar" -X POST "localhost:$PORT/api/sites" \
    -d '{"hostname":"load.example.com","honor_dnt":false}' >/dev/null
fi

cgroup() { # CPU seconds used so far, and memory in use (bytes)
  podman exec "$NAME" sh -c \
    'awk "/^usage_usec/{print \$2}" /sys/fs/cgroup/cpu.stat; cat /sys/fs/cgroup/memory.current'
}
now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }

echo "target=$TARGET cpus=$CPUS memory=$MEMORY synchronous=$SYNCHRONOUS image=$IMAGE (${SECONDS_PER_STEP}s per step)"
printf "%9s %8s %7s %8s %7s %7s %7s %8s %9s %7s\n" \
  offered/s done/s failed dropped p50_ms p95_ms p99_ms max_ms cpu_cores mem_MB
out=$(mktemp)
for rate in ${RATES//,/ }; do
  # CPU and memory over the middle of the step, while the load is steady.
  (
    sleep 1; a=($(cgroup)); ta=$(now)
    sleep $((SECONDS_PER_STEP - 3)); b=($(cgroup)); tb=$(now)
    echo "$(echo "scale=2; (${b[0]} - ${a[0]}) / 1000000 / ($tb - $ta)" | bc) $((${b[1]} / 1048576))" >"$out"
  ) &
  # shellcheck disable=SC2086
  # A wide port range and tw_reuse: with a connection per hit the generator
  # would otherwise run out of ports stuck in TIME_WAIT (~500 hits/s).
  line=$(podman run --rm --network "$NET" --ulimit nofile=65536:65536 \
    --sysctl net.ipv4.ip_local_port_range="1024 65535" --sysctl net.ipv4.tcp_tw_reuse=1 potoo:loadgen \
    --url "$URL" --seconds "$SECONDS_PER_STEP" --rates "$rate" \
    ${MODE_ARGS[@]+"${MODE_ARGS[@]}"} ${GENERATOR_ARGS:-} | tail -n 1)
  wait
  read -r cpu mem <"$out"
  printf "%s %9s %7s\n" "$line" "$cpu" "$mem"
done
rm -f "$out"

echo
echo "status:    $(podman ps -a --filter name=$NAME --format '{{.Status}}') (oom killed: $(podman inspect $NAME --format '{{.State.OOMKilled}}'))"
echo "peak mem:  $(($(podman exec "$NAME" cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo 0) / 1048576)) MB"
logs=$(podman logs "$NAME" 2>&1 || true)
if grep -q '^\[profile\]' <<<"$logs"; then
  echo; echo "phase timers of the profiling build (mean per hit, every 5 s):"
  grep '^\[profile\]' <<<"$logs"
fi
if [ "$TARGET" = goatcounter ]; then
  # GoatCounter buffers hits for 10 s; stopping it flushes them.
  podman stop -t 30 "$NAME" >/dev/null
  copy=$(mktemp -d)
  podman cp "$NAME:$DATA/gc.sqlite3" "$copy/" >/dev/null
  podman cp "$NAME:$DATA/gc.sqlite3-wal" "$copy/" >/dev/null 2>&1 || true
  echo "database:  $(ls -l "$copy/gc.sqlite3" | awk '{printf "%.0f MB", $5 / 1048576}')"
  echo "stored:    $(sqlite3 "$copy/gc.sqlite3" 'SELECT SUM(total) FROM hit_counts') pageviews counted (aggregated per path and hour, no raw hits)"
  rm -rf "$copy"
else
  echo "database:  $(podman exec "$NAME" ls -l /data/potoo.db | awk '{printf "%.0f MB", $5 / 1048576}')"
  echo "stored:    $(($(curl -s -b "$jar" "localhost:$PORT/api/export?site=1&days=1" | wc -l) - 1)) rows"
fi
