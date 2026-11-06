from lib import *
import lib, sys
cols, rows = (int(x) for x in (sys.argv[1] if len(sys.argv) > 1 else "80x24").split("x"))
lib.OUT = f"/tmp/claude-0/walk/out/ssh-{cols}x{rows}"

def clear_scene(t, tries=4):
    """If an event or fight is open, take the first open option / run (the walk is about the society screen)."""
    for _ in range(tries):
        txt = t.text()
        if "[1]" in txt and ("closed:" in txt or "Choose" in txt or "[2]" in txt) and "SOCIETY" not in txt:
            snap(t, "B-scene")
            t.send("1", quiet=1.2)
        elif "COMBAT" in txt:
            t.send("r", quiet=1.2)
        elif "Withdraw" in txt or "BOARDING" in txt:
            t.send("w", quiet=1.2)
        else:
            return

f = sign_in("founder", cols, rows)
open_lh(f)
snap(f, "B00-founder-after-three-weeks")
f.send("o", quiet=1.0)
snap(f, "B01-founder-convoy-gone-idle")
f.send("\x1b", quiet=0.8)
if f.has("Docked at"):
    f.send("l", quiet=1.2)
    for _ in range(3):
        f.send("B", quiet=0.8)
    snap(f, "B02-founder-borrows-for-the-outpost")
    f.send("\x1b", quiet=0.8)
    f.send("u", quiet=1.0)
f.send("v", quiet=1.0)
f.send(RIGHT, quiet=1.0)
snap(f, "B03-chart-cursor-on-a-charted-system-with-no-station")
f.send("\r", quiet=1.0)
snap(f, "B04-route-planner")
f.send("\r", quiet=1.5)
clear_scene(f)
snap(f, "B05-arrived")
f.send("o", quiet=1.0)
f.send(RIGHT, quiet=0.6); f.send(RIGHT, quiet=0.8)
snap(f, "B06-outposts-before-pledge")
f.send("P", quiet=1.0); f.send("P", quiet=1.0)
snap(f, "B07-pledged-ten-thousand")
for _ in range(4):
    f.send("p", quiet=0.8)
snap(f, "B08-pledged-twelve-thousand")
f.close()

b = sign_in("bob", cols, rows)
open_lh(b)
b.send("o", quiet=1.0)
b.send(RIGHT, quiet=0.6); b.send(RIGHT, quiet=0.8)
snap(b, "B09-bob-sees-the-site")
b.close()
print("done")
