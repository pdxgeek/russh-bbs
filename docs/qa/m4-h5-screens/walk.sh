#!/usr/bin/env bash
# Repeat the M4-H5 walk against a fresh board: ./walk.sh OUT_DIR COLSxROWS
# Needs the server built (cargo build -p russh-bbs-server), OpenSSH, faketime, sqlite3 and pyte.
set -euo pipefail
OUT=${1:?out dir}; SIZE=${2:-80x24}
HERE=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$HERE/../../.." && pwd)
WORK=$(mktemp -d); PORT=${PORT:-2241}; BIN=$ROOT/target/debug/russh-bbs-server
ssh-keygen -q -t ed25519 -N "" -f "$WORK/founder"; ssh-keygen -q -t ed25519 -N "" -f "$WORK/bob"
start() { RUSSH_BBS_REGISTRATION_MODE=open "$@" "$BIN" --listen 127.0.0.1:$PORT --db "$WORK/board.db" --host-key "$WORK/hostkey" >"$WORK/server.log" 2>&1 & echo $! >"$WORK/pid"; sleep 3; }
stop() { pkill -P "$(cat "$WORK/pid")" 2>/dev/null || true; kill "$(cat "$WORK/pid")" 2>/dev/null || true; pkill -f "listen 127.0.0.1:$PORT" 2>/dev/null || true; sleep 1; }
start env
TOKEN=$(grep -oE "[A-Z0-9]{4}(-[A-Z0-9]{4}){7}" "$WORK/server.log" | head -1)
python3 "$HERE/walk.py" claim $PORT "$WORK/founder" "$TOKEN" >/dev/null
python3 "$HERE/walk.py" join $PORT "$WORK/bob" >/dev/null
python3 "$HERE/walk.py" A $PORT "$WORK/founder" "$WORK/bob" "$OUT" "$SIZE"
stop
start faketime -f "+17d"
python3 "$HERE/walk.py" B $PORT "$WORK/founder" "$WORK/bob" "$OUT" "$SIZE"
stop
start faketime -f "+18d"
python3 "$HERE/walk.py" C $PORT "$WORK/founder" "$WORK/bob" "$OUT" "$SIZE"
stop
# The mystery stop starts from a prepared pupil (see walk_save.rs); everything after sign-in is real.
DAY=$(( $(date +%s) / 86400 + 20 ))
(cd "$ROOT" && HALLPASS_WALK_DAY=$DAY HALLPASS_WALK_OUT="$WORK/founder.hex" \
  cargo test -p russh-bbs-door-hallpass --test walk_save -- --ignored >/dev/null)
sqlite3 "$WORK/board.db" "UPDATE door_save SET data = x'$(cat "$WORK/founder.hex")' WHERE door = 'hallpass' AND account_id = 1;"
start faketime -f "+20d"
python3 "$HERE/walk.py" D $PORT "$WORK/founder" "$WORK/bob" "$OUT" "$SIZE"
stop
for c in "$OUT"/*.cells; do python3 "$ROOT/tools/screenshots/shot.py" "$c" "${c%.cells}.png" --scale 2 --quantize 64 >/dev/null; done
echo "done: $OUT (work dir $WORK)"
