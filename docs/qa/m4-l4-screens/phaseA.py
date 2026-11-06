from lib import *
import sys
cols, rows = (int(x) for x in (sys.argv[1] if len(sys.argv) > 1 else "80x24").split("x"))
global OUT
import lib
lib.OUT = f"/tmp/claude-0/walk/out/ssh-{cols}x{rows}"

def tab(t, n):
    for _ in range(n):
        t.send(RIGHT, quiet=0.6)

# --- founder: captain, society, convoy, colours ---
f = sign_in("founder", cols, rows)
open_lh(f)
snap(f, "A01-founder-intro")
f.send("\r", quiet=1.2)            # sign the loan, first background
snap(f, "A02-founder-bridge")
f.send("o", quiet=1.0)
snap(f, "A03-founder-society-no-convoy")
f.send("n", quiet=1.0)
snap(f, "A04-founder-forms-a-convoy")
f.send("3", quiet=1.0)             # quick note 3
f.send("t", quiet=0.6)
f.send("hold at the next dock, bob", quiet=0.6)
snap(f, "A05-founder-typing-a-line")
f.send("\r", quiet=1.2)
snap(f, "A06-founder-line-sent")
tab(f, 1)
snap(f, "A07-founder-piracy-before")
f.send("f", quiet=1.0)
snap(f, "A08-founder-colours-up")
tab(f, 1)
snap(f, "A09-founder-outposts-early")
tab(f, 1)
snap(f, "A10-founder-hall")
tab(f, 1)
snap(f, "A11-founder-journal")
f.close()

# --- bob: captain, joins the convoy, reads the log, colours ---
b = sign_in("bob", cols, rows)
open_lh(b)
b.send("\r", quiet=1.2)
b.send("o", quiet=1.0)
snap(b, "A12-bob-sees-the-open-convoy")
b.send("\r", quiet=1.2)
snap(b, "A13-bob-joined-and-reads-the-log")
tab(b, 1)
b.send("f", quiet=1.0)
snap(b, "A14-bob-colours-up-roster")
b.close()
print("done")
