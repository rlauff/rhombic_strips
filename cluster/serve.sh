#!/usr/bin/env bash
# Rhombic Strips compute helper: one command, no manual installation.
#
# The web page shows the exact command to paste; you never run this by hand
# unless you want to. Two flavours, same script:
#
#   This machine (native, all cores):
#     curl -fsSL https://raw.githubusercontent.com/rlauff/rhombic_strips/main/cluster/serve.sh | bash -s -- --local
#
#   Cluster (Slurm, through your own ssh login — the tunnel and the helper
#   live and die with this one terminal):
#     ssh -t -L 8642:127.0.0.1:8642 you@sshgate.math.tu-berlin.de \
#       'curl -fsSL https://raw.githubusercontent.com/rlauff/rhombic_strips/main/cluster/serve.sh | bash -s -- --partition=math'
#
# What it does, idempotently, all inside ~/.cache/rhombic_strips:
#   1. finds cargo (tries `module load rust`, then installs a minimal rustup
#      toolchain into your home if there is none — first run only),
#   2. clones or updates the repo and builds `strip_stream` headless
#      (no egui; a couple of minutes once, seconds afterwards),
#   3. prints a pairing code and starts the loopback relay (cluster: jobs go
#      through `srun`; --local: they run right here).
#
# Everything is per-user and temporary: Ctrl-C (or closing the ssh session)
# stops the relay and any running job; only the build cache stays for next
# time. Nothing listens on anything but 127.0.0.1.
#
# Options: --local  --partition=P  --time=HH:MM:SS  --cpus=N  --port=N

set -euo pipefail

REPO_URL="https://github.com/rlauff/rhombic_strips"
BASE="${RHOMBIC_HOME:-$HOME/.cache/rhombic_strips}"
PORT=8642
MODE=slurm
PARTITION="" TIMELIMIT="" CPUS=""

for arg in "$@"; do
  case "$arg" in
    --local)         MODE=direct ;;
    --partition=*)   PARTITION="${arg#*=}" ;;
    --time=*)        TIMELIMIT="${arg#*=}" ;;
    --cpus=*)        CPUS="${arg#*=}" ;;
    --port=*)        PORT="${arg#*=}" ;;
    *) echo "serve.sh: unknown option '$arg'" >&2; exit 2 ;;
  esac
done

say()  { printf '\033[1;36m» %s\033[0m\n' "$*"; }
warn() { printf '\033[1;33m» %s\033[0m\n' "$*"; }

command -v python3 >/dev/null 2>&1 || { echo "python3 is required" >&2; exit 1; }
command -v git     >/dev/null 2>&1 || { echo "git is required" >&2; exit 1; }

# -- 1. toolchain --------------------------------------------------------------

if ! command -v cargo >/dev/null 2>&1; then
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1 && command -v module >/dev/null 2>&1; then
  module load rust 2>/dev/null || module load cargo 2>/dev/null || true
fi
if ! command -v cargo >/dev/null 2>&1; then
  say "no Rust toolchain found — installing a minimal one into ~/.cargo (once)"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  . "$HOME/.cargo/env"
fi

# -- 2. sources & build ---------------------------------------------------------

mkdir -p "$BASE"
REPO="$BASE/repo"
if [ -d "$REPO/.git" ]; then
  say "updating $REPO"
  git -C "$REPO" pull --ff-only || warn "git pull failed — building the checked-out version"
else
  say "cloning $REPO_URL"
  git clone --depth 1 "$REPO_URL" "$REPO"
fi

say "building strip_stream (headless — first build takes a few minutes)"
cargo build --manifest-path "$REPO/Cargo.toml" --release \
  --bin strip_stream --no-default-features
BIN="$REPO/target/release/strip_stream"

# -- 3. pairing token ------------------------------------------------------------

TOKEN_FILE="$BASE/token"
if [ ! -s "$TOKEN_FILE" ]; then
  ( umask 077; python3 -c 'import secrets; print(secrets.token_hex(6))' > "$TOKEN_FILE" )
fi
TOKEN="$(cat "$TOKEN_FILE")"

# -- 4. mode ---------------------------------------------------------------------

SRUN_ARGS=""
if [ "$MODE" = slurm ]; then
  if command -v srun >/dev/null 2>&1; then
    [ -n "$PARTITION" ] && SRUN_ARGS="$SRUN_ARGS --partition=$PARTITION"
    [ -n "$TIMELIMIT" ] && SRUN_ARGS="$SRUN_ARGS --time=$TIMELIMIT"
    [ -n "$CPUS"      ] && SRUN_ARGS="$SRUN_ARGS --cpus-per-task=$CPUS"
  else
    warn "srun not found on this host — running jobs directly here instead"
    MODE=direct
  fi
fi

banner() {
  echo
  printf '\033[1m  Rhombic Strips helper is running (%s mode).\033[0m\n' "$MODE"
  printf '  Pairing code for the web page:  \033[1;35m%s\033[0m\n' "$TOKEN"
  echo   "  Leave this terminal open; Ctrl-C stops everything."
  echo
}

# Port already taken: most likely a helper from an earlier session. Print the
# code again and just keep this ssh tunnel alive instead of failing.
if python3 - "$PORT" <<'PY'
import socket, sys
s = socket.socket()
try:
    s.connect(("127.0.0.1", int(sys.argv[1]))); sys.exit(0)   # busy
except OSError:
    sys.exit(1)                                               # free
PY
then
  warn "a helper is already listening on port $PORT — reusing it"
  banner
  exec sleep infinity
fi

banner
RHOMBIC_TOKEN="$TOKEN" RHOMBIC_BIN="$BIN" RHOMBIC_MODE="$MODE" \
RHOMBIC_SRUN_ARGS="$SRUN_ARGS" RHOMBIC_PORT="$PORT" \
exec python3 "$REPO/cluster/relay.py"
