#!/usr/bin/env bash
#
# Run the TCGLense dev servers together:
#   API  -> http://localhost:8080  (api/,  cargo run)
#   Web  -> http://localhost:5173  (web/,  npm run dev)
#
# Both servers stream their output to this terminal (colors/HMR intact).
# Press Ctrl+C to stop both; if either server exits, the other is stopped too.
#
# By default both servers bind to localhost only. Pass --host to bind them to all
# interfaces (0.0.0.0) so you can open the app from another device on the same
# network (e.g. a phone at http://<this-machine-ip>:5173). See --help.

set -uo pipefail

# --- Argument parsing -------------------------------------------------------
# EXPOSE=true binds the servers to the LAN instead of localhost (see --host).
EXPOSE=false

usage() {
  cat <<'EOF'
Usage: scripts/dev.sh [--host] [--help]

Run the TCGLense dev servers together (API on :8080, web on :5173).

Options:
  --host      Expose both servers on your LAN (bind 0.0.0.0) so another device on
              the same network can reach them. Open the web app on the other
              device at http://<this-machine-ip>:5173. Default is localhost-only.
  -h, --help  Show this help and exit.
EOF
}

for arg in "$@"; do
  case "$arg" in
    --host | --lan | --expose) EXPOSE=true ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Error: unknown argument: $arg" >&2
      echo >&2
      usage >&2
      exit 1
      ;;
  esac
done

# Resolve the repo root from this script's own location, so it works from anywhere.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
API_DIR="$ROOT_DIR/api"
WEB_DIR="$ROOT_DIR/web"

# Tear down the whole process group (both servers and their children) on exit.
cleanup() {
  trap - INT TERM EXIT
  echo
  echo "Stopping dev servers..."
  kill 0 2>/dev/null
}
trap cleanup INT TERM EXIT

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Error: '$1' is not installed or not on PATH." >&2
    exit 1
  }
}
require cargo
require npm

# First-run niceties.
if [ ! -f "$API_DIR/.env" ] && [ -f "$API_DIR/.env.example" ]; then
  echo "-> Creating api/.env from .env.example (set a real JWT_SECRET before deploying)"
  cp "$API_DIR/.env.example" "$API_DIR/.env"
fi

if [ ! -d "$WEB_DIR/node_modules" ]; then
  echo "-> Installing web dependencies (npm install)..."
  (cd "$WEB_DIR" && npm install)
fi

# Refuse to start on top of a stale server. A leftover API/web process from an
# earlier session can keep serving old code while the new one appears to start
# fine (on macOS a 0.0.0.0 bind can even succeed alongside a stale 127.0.0.1
# listener on the same port), which makes for very confusing debugging.
for port in 8080 5173; do
  stale_pids="$(lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null || true)"
  if [ -n "$stale_pids" ]; then
    echo "Error: port $port is already in use by pid(s): $(echo "$stale_pids" | tr '\n' ' ')" >&2
    echo "Another dev server is likely still running; stop it first, e.g.:" >&2
    echo "  kill $(echo "$stale_pids" | tr '\n' ' ')" >&2
    exit 1
  fi
done

# Best-effort LAN IP so the --host banner can print a URL other devices can use.
# Tries macOS (ipconfig) then Linux (hostname -I); prints nothing if it can't tell.
lan_ip() {
  local ip=
  if command -v ipconfig >/dev/null 2>&1; then # macOS
    for iface in en0 en1 en2 en3; do
      ip="$(ipconfig getifaddr "$iface" 2>/dev/null)" || ip=
      [ -n "$ip" ] && { printf '%s\n' "$ip"; return 0; }
    done
  fi
  if command -v hostname >/dev/null 2>&1; then # Linux
    ip="$(hostname -I 2>/dev/null | awk '{print $1}')" || ip=
    [ -n "$ip" ] && { printf '%s\n' "$ip"; return 0; }
  fi
  return 1
}

if [ "$EXPOSE" = true ]; then
  ip="$(lan_ip || true)"
  echo "Starting TCGLense dev servers on your LAN (Ctrl+C to stop both):"
  if [ -n "$ip" ]; then
    echo "  API  -> http://$ip:8080   (also http://localhost:8080)"
    echo "  Web  -> http://$ip:5173   (also http://localhost:5173)   <- open this on your other device"
  else
    echo "  API  -> http://0.0.0.0:8080   (also http://localhost:8080)"
    echo "  Web  -> http://0.0.0.0:5173   (also http://localhost:5173)"
    echo "  (couldn't auto-detect this machine's LAN IP; try 'ipconfig getifaddr en0')"
  fi
  echo "  Both devices must be on the same network; allow the macOS firewall prompt if it appears."
else
  echo "Starting TCGLense dev servers (Ctrl+C to stop both):"
  echo "  API  -> http://localhost:8080"
  echo "  Web  -> http://localhost:5173"
fi
echo

# With --host, bind both servers to every interface. HOST is read by the API
# (see api/src/config.rs); --host is Vite's flag. The web server is the one you
# browse to — it proxies /api to the API over localhost — so exposing the API too
# is only needed to hit it directly from another device. These are fixed literals,
# so the unquoted expansion below is intentional word-splitting (and stays empty,
# adding no argument, in the default localhost mode — required for bash 3.2).
API_HOST_ENV=
WEB_HOST_ARGS=
if [ "$EXPOSE" = true ]; then
  API_HOST_ENV="HOST=0.0.0.0"
  WEB_HOST_ARGS="-- --host"
fi

# Start both in the background; exec so $! is the real server process.
# shellcheck disable=SC2086  # intentional word-splitting of the host flags above
(cd "$API_DIR" && exec env $API_HOST_ENV cargo run) &
API_PID=$!
# shellcheck disable=SC2086
(cd "$WEB_DIR" && exec npm run dev $WEB_HOST_ARGS) &
WEB_PID=$!

# Stay up while both are alive; as soon as either stops, fall through to cleanup.
while kill -0 "$API_PID" 2>/dev/null && kill -0 "$WEB_PID" 2>/dev/null; do
  sleep 1
done
