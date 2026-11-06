import sys, re, importlib.util
sp = importlib.util.spec_from_file_location("s4walk", "/home/user/russh-bbs/docs/qa/m4-s4-screens/walk.py")
s4 = importlib.util.module_from_spec(sp); sp.loader.exec_module(s4)
Term = s4.Term
port = int(sys.argv[1]); user = sys.argv[2]
c = Term(port, f"/tmp/qa/keys/{user}", 80, 24, user=user)
s4.found_or_report(c); s4.dismiss_report(c)
s4.court(c); s4.room(c, "The post")
for i in range(1, 9):
    ok1 = c.press("Write a letter")
    ok2 = c.press("Send the letter to")
    msg = [l.strip("│ ").strip() for l in c.text().splitlines() if l.startswith("│ ") and not l.startswith("│ »") and l.strip("│ ").strip()][-1:]
    print(i, ok1, ok2, "|", msg[-1][:90] if msg else None, flush=True)
c.close()
