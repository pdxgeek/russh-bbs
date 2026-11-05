#!/bin/bash
# usage: start.sh DATADIR PORT TELNETPORT [extra env...]
D=$1; P=$2; T=$3; shift 3
cd $D
env RUSSH_BBS_LISTEN=127.0.0.1:$P RUSSH_BBS_DB=$D/bbs.db RUSSH_BBS_HOST_KEY=$D/host_key \
 RUSSH_BBS_TELNET_ENABLED=true RUSSH_BBS_TELNET_ALLOW_LOGIN=true RUSSH_BBS_TELNET_LISTEN=127.0.0.1:$T \
 RUSSH_BBS_IP_BURST=100000 RUSSH_BBS_KEY_BURST=100000 RUSSH_BBS_IP_INTERVAL_SECS=1 \
 RUST_LOG=info "$@" /home/user/russh-bbs/target/release/russh-bbs-server > $D/server.log 2>&1 &
echo $! > $D/pid
