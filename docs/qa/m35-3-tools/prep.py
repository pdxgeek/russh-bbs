import sys, time, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import *
port = sys.argv[1]
s = Sess("prep", f"ssh -tt -i /tmp/qa/keys/founder -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR founder@127.0.0.1", 80, 24, {"TERM": "xterm-256color", "COLORTERM": "truecolor"}, outdir="/tmp/qa/raw")
s.settle(2.5)
print("1:", s.has("Any key: continue"))
s.key(" ", 1.0)
print("2:", s.has("look like icons"))
s.key("n", 0.5); s.key("y", 1.0)
print("3:", s.has("Main menu"))
s.key("l", 1.0)
subprocess.run(["tmux", "kill-session", "-t", "prep"])
