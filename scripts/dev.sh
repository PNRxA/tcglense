#!/usr/bin/env bash
#
# Run the TCGLense dev servers together:
#   API  -> http://localhost:8080  (api/,  cargo run)
#   Web  -> http://localhost:5173  (web/,  npm run dev)
#
# Both servers stream their output to this terminal (colors/HMR intact).
# Press Ctrl+C to stop both; if either server exits, the other is stopped too.

set -uo pipefail

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

echo "Starting TCGLense dev servers (Ctrl+C to stop both):"
echo "  API  -> http://localhost:8080"
echo "  Web  -> http://localhost:5173"
echo

# Start both in the background; exec so $! is the real server process.
(cd "$API_DIR" && exec cargo run) &
API_PID=$!
(cd "$WEB_DIR" && exec npm run dev) &
WEB_PID=$!

# Stay up while both are alive; as soon as either stops, fall through to cleanup.
while kill -0 "$API_PID" 2>/dev/null && kill -0 "$WEB_PID" 2>/dev/null; do
  sleep 1
done
