import sys, time
sys.path.insert(0, "/home/user/russh-bbs/docs/qa/m4-s3-screens")
from walk import Term
mode = sys.argv[1]
if mode == "claim":
    t = Term(22222, "founder_key", 100, 30, user="claim")
    t.pump(2.0)
    print(t.text()); t.send(sys.argv[2] + "\r", quiet=1.5); print(t.text())
    t.send("walkpassword-aaa-bbb\r", quiet=1.0); print(t.text())
    t.send("walkpassword-aaa-bbb\r", quiet=2.0); print(t.text())
    t.close()
else:
    t = Term(22222, "bob_key", 100, 30, user="new")
    t.pump(2.5); print(t.text())
    for k in sys.argv[2:]:
        t.send(k.encode().decode("unicode_escape"), quiet=1.2); print("-----", repr(k)); print(t.text())
    t.close()
