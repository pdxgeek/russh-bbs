import sys
sys.path.insert(0, "/tmp/qa")
from lib import *
tr, port, user = sys.argv[1:4]
pw = os.environ["QA_PW_PREFIX"] + user
if tr == "telnet": cmd = f"python3 /tmp/qa/telnetc.py 127.0.0.1 {port} 80 24 xterm-256color /tmp/qa/rx.wire"
else: cmd = f"ssh -tt -i /tmp/qa/keys/{user} -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR {user}@127.0.0.1"
s = Sess("rx", cmd, 80, 24, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw"); s.settle(2.5)
if tr == "telnet": s.key(user + "\r", 1); s.key(pw + "\r", 2.5)
s.key(" ", 0.8); s.key("d", 0.6); rm = len(s.tal.rev)
for g in "123456":
    s.key(g, 1.0); s.key("\r", 0.5); s.key("\x1b", 0.5); s.key("\x1b", 0.5)
    if not s.has("Doors"): s.key("d", 0.6)
print(tr, s.tal.summary())
s.close()
