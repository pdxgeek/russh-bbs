import sys, os; sys.path.insert(0, ".")
from lib import *
def ssh_sess(name, port, user, cols=80, rows=24, tier="nerd", color=True, extra="", env=None, ct=True):
    s = Sess(name, f"ssh -tt -i /tmp/qa/keys/{user} -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR {'-o SetEnv=COLORTERM=truecolor' if ct else ''} {extra} {user}@127.0.0.1", cols, rows, {"TERM": "xterm-256color", **(env or {})}, outdir="/tmp/qa/raw")
    s.settle(2.5); s.key(" ", 0.8)
    if s.has("Icon check"):
        for k in {"nerd": ["y"], "unicode": ["n", "y"], "ascii": ["n", "n"]}[tier]: s.key(k, 0.5)
    elif s.has("Any key"): s.key(" ", 0.6)
    # Always set the tier explicitly: the icon check appears only once per account.
    if not s.has("Main menu"): s.key(" ", 0.5)
    s.key("s", 0.5); s.key("i", 0.5)
    for k in {"nerd": ["y"], "unicode": ["n", "y"], "ascii": ["n", "n"]}[tier]: s.key(k, 0.5)
    if s.has("Icon check"): s.key("\x1b", 0.4)
    want = "on" if (tier != "ascii" or os.environ.get("QA_COLOR") == "on") else "off"
    if f"Color: {want}" not in s.text(): s.key("c", 0.4)
    s.key("\x1b", 0.6)
    return s
def keys(s, seq, wait=0.5):
    for k in seq:
        if k == "ESC": s.key("\x1b", wait)
        elif k == "UP": s.key("\x1b[A", wait)
        elif k == "DOWN": s.key("\x1b[B", wait)
        elif k == "ENTER": s.key("\r", wait)
        elif k == "TAB": s.key("\t", wait)
        else: s.key(k, wait)
if __name__ == "__main__":
    port, user, seq = sys.argv[1], sys.argv[2], sys.argv[3].split(",")
    s = ssh_sess("drv", port, user, tier=sys.argv[4] if len(sys.argv) > 4 else "nerd")
    for k in seq:
        keys(s, [k]); print(f"--- after {k!r}"); print(s.text())
    s.close()
