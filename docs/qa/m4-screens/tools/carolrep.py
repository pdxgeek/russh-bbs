import sys, importlib.util
sp = importlib.util.spec_from_file_location("s4walk", "/home/user/russh-bbs/docs/qa/m4-s4-screens/walk.py")
s4 = importlib.util.module_from_spec(sp); sp.loader.exec_module(s4)
c = s4.Term(int(sys.argv[1]), f"/tmp/qa/keys/{sys.argv[2]}", 80, 24, user=sys.argv[2])
s4.found_or_report(c)
print("\n".join(l.rstrip() for l in c.text().splitlines() if l.strip())[:2400])
c.send(" ", quiet=1.0)
print("\n".join(l.rstrip() for l in c.text().splitlines() if l.strip())[:600])
c.close()
