#!/bin/bash
# usage: run.sh BIN LABEL  -> fresh board, claim, then throttled sessions; writes /tmp/qa/out-LABEL.txt
BIN=$1; LABEL=$2; S=$(cd "$(dirname "$0")" && pwd)
D=/tmp/qa/board-$LABEL; rm -rf $D; mkdir -p $D
P=3200; export QA_FOUNDER_PASS="founder pass phrase one"
cd $D
env RUSSH_BBS_LISTEN=127.0.0.1:$P RUSSH_BBS_DB=$D/bbs.db RUSSH_BBS_HOST_KEY=$D/host_key \
 RUSSH_BBS_IP_BURST=100000 RUSSH_BBS_KEY_BURST=100000 RUSSH_BBS_IP_INTERVAL_SECS=1 \
 RUST_LOG=info "$BIN" > $D/server.log 2>&1 &
echo $! > $D/pid
sleep 2
TOK=$(grep -E '^    ' $D/server.log | head -1 | tr -d ' ')
echo "token=$TOK"
cd $S && python3 claim.py $P "$TOK" > $D/claim.txt 2>&1
tail -5 $D/claim.txt
