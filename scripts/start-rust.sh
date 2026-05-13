#!/usr/bin/env bash
# Start the Rust PreXiv server from a production checkout.
#
# This script is intentionally secret-aware but secret-free: it sources the
# deployment-local .env file if present, and never prints its contents.

set -euo pipefail

REPO="${REPO:-$HOME/prexiv-deploy/prexiv}"
DATA_DIR="${DATA_DIR:-/var/lib/prexiv/current}"
PID_FILE="${PID_FILE:-$HOME/prexiv-deploy/prexiv-rust.pid}"
LOG_FILE="${LOG_FILE:-$HOME/prexiv-deploy/prexiv-rust.log}"

cd "$REPO"

if [ -r "$REPO/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$REPO/.env"
  set +a
fi

# SMTP credentials for outbound verification mail.
# Live secret file (not in git), typically mode 0640 root:dbai.
if [ -r /etc/prexiv/smtp.env ]; then
  set -a
  # shellcheck disable=SC1091
  . /etc/prexiv/smtp.env
  set +a
fi

export PORT="${PORT:-3000}"
export DATA_DIR="${DATA_DIR:-/var/lib/prexiv/current}"
export UPLOAD_DIR="${UPLOAD_DIR:-$DATA_DIR/uploads}"
export APP_URL="${APP_URL:-https://victoria.tail921ea4.ts.net}"
export NODE_ENV="${NODE_ENV:-production}"
export RUST_LOG="${RUST_LOG:-info,sqlx=warn,tower_http=info}"

nohup "$REPO/rust/target/release/prexiv" > "$LOG_FILE" 2>&1 < /dev/null &
echo $! > "$PID_FILE"
