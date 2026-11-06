import sys, re, importlib.util
sp = importlib.util.spec_from_file_location("s4walk", "/home/user/russh-bbs/docs/qa/m4-s4-screens/walk.py")
s4 = importlib.util.module_from_spec(sp); sp.loader.exec_module(s4)
port = int(sys.argv[1]); user = sys.argv[2]
c = s4.Term(port, f"/tmp/qa/keys/{user}", 80, 24, user=user)
s4.found_or_report(c); print(c.text()[:0]); s4.dismiss_report(c)
s4.court(c); s4.room(c, "The post")
print("\n".join(l.rstrip() for l in c.text().splitlines() if l.strip())[:2600])
c.close()
