#!/bin/bash
# usage: restart.sh +22d
cd /tmp/claude-0/walk/board
pkill -f russh-bbs-server; sleep 1
(RUSSH_BBS_REGISTRATION_MODE=open nohup faketime -f "$1" /home/user/russh-bbs/target/debug/russh-bbs-server --listen 127.0.0.1:22222 --host-key host_key --db board.db >> server.log 2>&1 &)
sleep 4
tail -3 server.log | sed 's/\x1b\[[0-9;]*m//g' | cut -c1-160
