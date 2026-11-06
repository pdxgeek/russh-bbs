#!/bin/bash
# run_walk.sh s4|h4  -- the slice authors' two-account walk on a fresh board, release binary, faketime phases
K=$1; BIN=/home/user/russh-bbs/target/release/russh-bbs-server
if [ $K = s4 ]; then P=2851; W=/home/user/russh-bbs/docs/qa/m4-s4-screens/walk.py; PH="A B C"; OFF="0 8 9"; else P=2861; W=/home/user/russh-bbs/docs/qa/m4-h4-screens/walk.py; PH="A B"; OFF="0 11"; fi
D=/tmp/qa/d/${K}n; rm -rf $D; mkdir -p $D; OUT=/tmp/qa/out/${K}n
start() { ( cd $D; env RUSSH_BBS_LISTEN=127.0.0.1:$P RUSSH_BBS_DB=$D/bbs.db RUSSH_BBS_HOST_KEY=$D/host_key RUSSH_BBS_REGISTRATION_MODE=open RUSSH_BBS_IP_BURST=100000 RUSSH_BBS_KEY_BURST=100000 "$@" > $D/server.log 2>&1 & ); sleep 3; }
stop() { for p in $(pgrep -f russh-bbs-server); do tr '\0' ' ' < /proc/$p/environ | grep -q "LISTEN=127.0.0.1:$P" && kill $p; done; sleep 2; }
start $BIN
if [ $K = s4 ]; then
  TOK=$(sed -n '/SETUP TOKEN/,/end of setup/p' $D/server.log | sed -n 8p | tr -d ' ')
  python3 /tmp/qa/claim.py $P $TOK >/dev/null 2>&1; (cd /tmp/qa; python3 join.py $P bob >/dev/null 2>&1)
else
  TOK=$(grep -oE "[A-Z0-9]{4}(-[A-Z0-9]{4}){7}" $D/server.log | head -1)
  python3 $W claim $P /tmp/qa/keys/founder $TOK >/dev/null; python3 $W join $P /tmp/qa/keys/bob >/dev/null
fi
python3 -c "
import sqlite3; c=sqlite3.connect('$D/bbs.db',timeout=20); c.execute(\"update role_limit set value=NULL where role in ('member','trusted') and kind in ('calls_per_day','minutes_per_day')\"); c.commit()"
set -- $OFF; i=0
for ph in $PH; do
  off=$(echo $OFF | cut -d' ' -f$((i+1))); i=$((i+1))
  if [ "$off" != 0 ]; then stop; start faketime -f "+${off}d" $BIN; fi
  python3 $W $ph $P /tmp/qa/keys/founder /tmp/qa/keys/bob $OUT/$ph 80x24
done
stop; echo $K done
