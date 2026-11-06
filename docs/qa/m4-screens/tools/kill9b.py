import sys, re, os, signal, subprocess, time
sys.path.insert(0, "/tmp/qa")
from lib import *
from drive import *
port, user, phase = sys.argv[1], sys.argv[2], sys.argv[3]
s = ssh_sess("k9", port, user, 80, 24, tier="unicode")
s.key("d", 0.6); s.key("1", 1.0)
def left(): 
    m = re.search(r"Tosses left today: (\d+)", s.text()); return m.group(1) if m else None
print("before", left())
if phase == "play":
    for k in "hth": s.key(k, 0.7); s.key("\r", 0.5) if "any key" in s.text().lower() else None
    print("after 3 calls", left())
    # SIGKILL the server for this port NOW, session still open
    for p in subprocess.run(["pgrep", "-f", "russh-bbs-server"], capture_output=True, text=True).stdout.split():
        try:
            if f"LISTEN=127.0.0.1:{port}" in open(f"/proc/{p}/environ").read().replace("\0", " "): os.kill(int(p), signal.SIGKILL); print("killed", p)
        except Exception as e: pass
s.close()
