from lib import *
import re
def now(t):
    t.send("s",quiet=1.0)
    if not t.has("Next layout"): t.send("s",quiet=1.0)
    tx=t.text(); th=re.search(r"Theme: (\S+)",tx); la=re.search(r"Layout: (\S+)",tx)
    t.send("\x1b",quiet=1.0); return (th.group(1) if th else "?", la.group(1) if la else "?")
for (cols,rows,style,layout) in [(80,24,"neon","dashboard"),(80,24,"phosphor","classic"),(100,30,"paper","wide"),(80,24,"amber","split")]:
    t=term("founder",cols,rows); t.pump(2.0); settle(t,"Main menu")
    a=set_choice(t,"Theme",style,"t"); b=set_choice(t,"Layout",layout,"l"); t.pump(2.0)
    got=now(t); print(style,layout,a,b,got)
    tag=f"{got[0]}-{got[1]}-{cols}x{rows}"
    snap(t,f"30-home-{tag}")
    t.send("f",quiet=1.0); t.send("1",quiet=1.0); snap(t,f"31-threads-{tag}")
    t.send("1",quiet=1.0); snap(t,f"32-reader-{tag}")
    for _ in range(3): t.send("\x1b",quiet=0.7)
    at_menu(t)
    t.send("c",quiet=1.0); t.send("\r",quiet=1.0); snap(t,f"33-chat-{tag}")
    for _ in range(2): t.send("\x1b",quiet=0.6)
    at_menu(t)
    t.send("m",quiet=1.0); snap(t,f"34-mail-{tag}"); t.send("\x1b",quiet=0.6); at_menu(t)
    t.send("y",quiet=0.8); t.send("p",quiet=1.0); snap(t,f"35-polls-{tag}")
    t.close()
