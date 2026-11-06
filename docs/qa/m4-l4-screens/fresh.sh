#!/bin/bash
# usage: fresh.sh   -> new board, claim founder, create bob, raise limits
cd /tmp/claude-0/walk
pkill -f russh-bbs-server; sleep 1
rm -rf board; mkdir board
cd board
(RUSSH_BBS_REGISTRATION_MODE=open nohup /home/user/russh-bbs/target/debug/russh-bbs-server --listen 127.0.0.1:22222 --host-key host_key --db board.db > server.log 2>&1 &)
sleep 3
TOKEN=$(sed 's/\x1b\[[0-9;]*m//g' server.log | grep -A9 "SETUP TOKEN" | grep -oE "[A-Z0-9]{4}(-[A-Z0-9]{4}){7}" | head -1)
cd ..
python3 explore.py claim "$TOKEN" > /dev/null 2>&1
python3 explore.py new n 'bob\r' 'bobpassword-123\r' 'bobpassword-123\r' '\r' > /dev/null 2>&1
python3 - <<'PY'
import sqlite3
c=sqlite3.connect('/tmp/claude-0/walk/board/board.db')
c.execute("update role_limit set value=100000 where kind in ('calls_per_day','minutes_per_day','max_sessions')")
c.commit()
PY
echo ready
