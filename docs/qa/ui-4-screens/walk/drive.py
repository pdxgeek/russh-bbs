import os, sys, time
S="/tmp/claude-0/-home-user-russh-bbs/bdc9f6bb-ee06-5437-83d4-9e11c0ff9699/scratchpad"
sys.path.insert(0, S+"/pylib")
sys.path.insert(0, "/home/user/russh-bbs/docs/qa/m4-s3-screens")
from walk import Term, claim
PORT=2290
W=S+"/walk"
def key(u): return f"{W}/{u}"
if sys.argv[1]=="claim":
    claim(PORT, key("founder"), sys.argv[2])
elif sys.argv[1]=="peek":
    t=Term(PORT, key(sys.argv[2]), 100, 30, user=sys.argv[2]); t.pump(2.5); print(t.text()); t.close()
