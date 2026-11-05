#!/bin/bash
# setup2.sh NAME PORT TPORT SEEDKIND(none|benign|hostile) [env...]
N=$1; P=$2; T=$3; K=$4; shift 4
D=/tmp/qa/d2/$N; mkdir -p $D
/tmp/qa/start.sh $D $P $T "$@"; sleep 2
TOK=$(sed -n '/SETUP TOKEN/,/end of setup/p' $D/server.log | sed -n 8p | tr -d ' ')
python3 /tmp/qa/claim.py $P $TOK > /dev/null 2>&1
kill $(cat $D/pid); sleep 1.5
python3 - <<P
import sqlite3
c=sqlite3.connect("$D/bbs.db", timeout=20)
c.execute("update role_limit set value=NULL where role in ('member','trusted') and kind in ('calls_per_day','minutes_per_day')"); c.commit()
P
[ "$K" != none ] && python3 /tmp/qa/seed.py $D/bbs.db $K
/tmp/qa/start.sh $D $P $T "$@"; sleep 2; echo "$N up on $P/$T"
