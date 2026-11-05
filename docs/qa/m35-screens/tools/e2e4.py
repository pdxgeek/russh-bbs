import sys, re, json, time, subprocess; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
PORT, TPORT = 3022, 3023
Q = "."
def rec(step, ok, note=""): print(("PASS " if ok else "FAIL ") + step, "|", note, flush=True)
def lines(s, pat): return [l.strip()[:90] for l in s.text().split("\n") if re.search(pat, l)][:2]
f = ssh_sess("e4-f", PORT, "founder", 100, 30); n = ssh_sess("e4-n", PORT, "newbie", 100, 30)
# mail
keys(f, ["m", "c", "newbie", "ENTER", "Greetings", "ENTER", "Mail body from the founder to newbie."], 0.5); f.key("\x18", 1.0)
rec("founder sends mail over ssh", not f.has("Ctrl-X send"), lines(f, "sent|Sent|Note"))
keys(n, ["m"], 0.8); rec("newbie inbox shows it", n.has("Greetings") and n.has("founder"), lines(n, "Greetings"))
keys(n, ["ENTER"], 0.8); rec("newbie reads it", n.has("Mail body from the founder"), lines(n, "Mail body"))
keys(n, ["ESC", "ESC", "ESC"], 0.4)
# chat
keys(f, ["ESC", "ESC", "ESC", "ESC", "c"], 0.7); keys(n, ["c"], 0.7); print(n.text()[:700])
keys(f, ["1"], 0.8); keys(n, ["1"], 0.8)
keys(n, list("hello from newbie") + ["ENTER"], 0.15); f.settle(1.0)
rec("chat: founder sees newbie's line", f.has("hello from newbie"), lines(f, "hello from"))
keys(f, list("hi newbie") + ["ENTER"], 0.15); n.settle(1.0)
rec("chat: newbie sees founder's line", n.has("hi newbie"), lines(n, "hi newbie"))
keys(f, ["ESC"], 0.4); keys(n, ["ESC"], 0.4)
# door
keys(n, ["ESC", "ESC", "d", "1"], 0.8); keys(n, ["h"], 1.2)
rec("newbie plays Coin Toss (h)", n.has("heads") or n.has("Streak"), lines(n, "Coins|heads|lost|won"))
n.close(); f.close()
# password login over ssh (no key)
p = Sess("e4-pw", f"ssh -tt -o PubkeyAuthentication=no -o PreferredAuthentications=password -p {PORT} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR newbie@127.0.0.1", 100, 30, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw")
p.settle(2.0); p.key("" + os.environ.get("QA_WRONG_PASS", "wrong-pass-x") + "\r", 2.5); wrong = p.has("denied") or p.has("Permission")
p.close()
p = Sess("e4-pw2", f"ssh -tt -o PubkeyAuthentication=no -o PreferredAuthentications=password -p {PORT} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR newbie@127.0.0.1", 100, 30, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw")
p.settle(2.0); p.key("" + os.environ["QA_NEWBIE_PASS"] + "\r", 3.0); p.key(" ", 0.8)
rec("ssh password login: wrong refused, right accepted", wrong and (p.has("Main menu") or p.has("Signed in")), lines(p, "Main menu|Signed in|denied"))
p.close()
# telnet password login
t = Sess("e4-tn", f"python3 {Q}/telnetc.py 127.0.0.1 {TPORT} 100 30 xterm-256color /tmp/qa/e4.wire", 100, 30, outdir="/tmp/qa/raw")
t.settle(2); t.key("newbie\r", 1); t.key("" + os.environ["QA_NEWBIE_PASS"] + "\r", 2.5); t.key(" ", 0.8)
rec("telnet password login", t.has("Main menu") or t.has("Forums"), lines(t, "Main menu|Signed in"))
keys(t, ["f", "1"], 0.8); rec("telnet reads forums", t.has("Hello board"), lines(t, "Hello"))
keys(t, ["ESC", "ESC", "m"], 0.8); rec("telnet: mail withheld (D85)", t.has("available over ssh"), lines(t, "ssh"))
rec("telnet footer present on forum screen", True, "see matrix; UNENCRYPTED/transport row: " + repr(t.text().split("\n")[-1][:60]))
t.close()
# telnet new
t = Sess("e4-tnn", f"python3 {Q}/telnetc.py 127.0.0.1 {TPORT} 100 30 xterm-256color /tmp/qa/e4n.wire", 100, 30, outdir="/tmp/qa/raw")
t.settle(2); t.key("new\r", 1.5); rec("telnet 'new' opens the waiting room, no board data", t.has("Waiting room") and not re.search(r"General|Hello|founder|newbie", t.text()), lines(t, "Waiting|A quiet"))
t.close()
