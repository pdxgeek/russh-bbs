#!/bin/bash
pkill -f "russh-bbs-server --db" ; sleep 1
cd /tmp/claude-0/-home-user-russh-bbs/bdc9f6bb-ee06-5437-83d4-9e11c0ff9699/scratchpad/walk
rm -f bbs.db* host_key* server.log /root/.local/share/russh-bbs/mail.key
(RUSSH_BBS_LISTEN=127.0.0.1:2290 RUSSH_BBS_REGISTRATION_MODE=open RUSSH_BBS_BOARD_NAME="Night Owl BBS" RUSSH_BBS_BOARD_LOCATION="Portland, OR" RUSSH_BBS_BOARD_SINCE=2026 env "$@" nohup /home/user/russh-bbs/target/debug/russh-bbs-server --db $PWD/bbs.db --host-key $PWD/host_key > server.log 2>&1 &)
sleep 3
grep -A1 "^    [A-Z0-9]" server.log | head -1 | tr -d ' '
