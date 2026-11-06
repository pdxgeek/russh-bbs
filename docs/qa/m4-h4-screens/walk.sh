#!/usr/bin/env bash
# Repeat the Hallpass social walk against a fresh board: ./walk.sh OUT_DIR COLSxROWS
# Needs the server built (cargo build -p russh-bbs-server), OpenSSH, faketime and pyte.
set -euo pipefail
OUT=${1:?out dir}; SIZE=${2:-80x24}
HERE=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$HERE/../../.." && pwd)
WORK=$(mktemp -d); PORT=${PORT:-2231}; BIN=$ROOT/target/debug/russh-bbs-server
ssh-keygen -q -t ed25519 -N "" -f "$WORK/founder"; ssh-keygen -q -t ed25519 -N "" -f "$WORK/bob"
start() { RUSSH_BBS_REGISTRATION_MODE=open "$@" "$BIN" --listen 127.0.0.1:$PORT --db "$WORK/board.db" --host-key "$WORK/hostkey" >"$WORK/server.log" 2>&1 & echo $! >"$WORK/pid"; sleep 3; }
stop() { kill "$(cat "$WORK/pid")" 2>/dev/null || true; sleep 1; }
start env
TOKEN=$(grep -oE "[A-Z0-9]{4}(-[A-Z0-9]{4}){7}" "$WORK/server.log" | head -1)
python3 "$HERE/walk.py" claim $PORT "$WORK/founder" "$TOKEN" >/dev/null
python3 "$HERE/walk.py" join $PORT "$WORK/bob" >/dev/null
python3 "$HERE/walk.py" A $PORT "$WORK/founder" "$WORK/bob" "$OUT" "$SIZE"
stop
start faketime -f "+11d"
python3 "$HERE/walk.py" B $PORT "$WORK/founder" "$WORK/bob" "$OUT" "$SIZE"
stop
echo "done: $OUT (work dir $WORK)"
