import sys; sys.path.insert(0, ".")
from lib import *
port, tok = sys.argv[1], sys.argv[2]
s = Sess("claim", f"ssh -tt -i /tmp/qa/keys/founder -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR claim@127.0.0.1", 100, 30, {"TERM": "xterm-256color"})
s.settle(1.5); print(s.text())
s.key(tok + "\r", 1.5); print(s.text())
s.key("" + os.environ["QA_FOUNDER_PASS"] + "\r", 1.5); s.key("" + os.environ["QA_FOUNDER_PASS"] + "\r", 2.5); print(s.text()); s.close()
