#!/bin/bash
run() { # user size tier layout theme
  python3 ./walk_h.py hostile 2522 $1 $2 $3 full $4 $5 >> /tmp/qa/hw/log2.txt 2>&1 &
  python3 ./walk_h.py benign 2622 $1 $2 $3 full $4 $5 >> /tmp/qa/hw/log2.txt 2>&1 &
  wait
}
for u in bob founder; do for sz in 80x24 132x43; do for t in ascii unicode; do run $u $sz $t dashboard default; done; done; done
QA_COLOR=on run bob 80x24 ascii dashboard default
for L in focus split compact lightbar classic; do for sz in 80x24 132x43; do run bob $sz nerd $L neon; done; done
for T in phosphor paper amber deepspace ansi contrast; do run bob 80x24 unicode wide $T; done
echo ALLDONE >> /tmp/qa/hw/log2.txt
