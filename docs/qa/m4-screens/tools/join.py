import sys, os; sys.path.insert(0,"/tmp/qa")
from lib import *
port, user = sys.argv[1], sys.argv[2]
pw = os.environ["QA_PW_PREFIX"] + user
s = Sess("join-"+user, f"ssh -tt -i /tmp/qa/keys/{user} -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR new@127.0.0.1", 100, 30, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw")
s.settle(4); print(s.text())
for k in [" ", "n"]:
    s.key(k, 1.2); print("----", repr(k)); print(s.text())
s.key(user+"\r", 2.0); print(s.text())
s.key(pw+"\r", 1.2); s.key(pw+"\r", 2.5); print(s.text())
for _ in range(3):
    s.key("\r", 1.0)
print(s.text()); s.close()
