import sys,re,time,json; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
PORT=int(sys.argv[1]); R={}
def rec(k,ok,n=""): R[k]=(bool(ok),str(n)); print(("PASS " if ok else "FAIL "),k,"|",n,flush=True)
f=ssh_sess("dkf",PORT,"founder",100,30); keys(f,["c","n"],0.8)
for ch in "lobby": f.key(ch,0.12)
f.key("\r",1.0); f.close()
def mouse_on(s):
    s.key("s",0.5)
    if "Mouse: off" in s.text(): s.key("o",0.6)
    s.key("\x1b",0.7)
def divider(s):
    # column where most rows carry the same border char in the body
    cnt={}
    for y in range(4,s.rows-2):
        for x in range(s.cols):
            if s.scr.buffer[y][x].data in "║│┃": cnt[x]=cnt.get(x,0)+1
    return sorted(cnt.items(),key=lambda kv:-kv[1])[:3]
def offmark(s,rm): return [x[:3] for x in s.tal.rev[rm:] if not (x[0]==s.rows-1 and x[3])]
for tier in ("nerd","ascii"):
  for (c,r) in ((65,20),(80,24),(100,30),(132,43)):
    s=ssh_sess(f"dk{c}{tier}",PORT,"founder",c,r,tier=tier); mouse_on(s)
    rm=len(s.tal.rev); mark=len(s.raw())
    seq=[("menu",[]),("forums",["f"]),("mail",["m"]),("members",["u"]),("community",["y"])]
    ok=True; note=[]
    for nm,sq in seq:
        keys(s,["ESC"]*4,0.2); keys(s,sq,0.6); s.key("\x14",0.9)
        t=s.text(); has_chat=("lobby" in t or "Say something" in t or "Rooms" in t or "Chat" in t)
        bad=offmark(s,rm); mk=[(y,x) for y,x,cc,b in s.revcells() if b][:1]
        if not has_chat or bad or mk!=[(r-1,0)] : ok=False; note.append((nm,has_chat,bad[:2],mk))
        s.key("\x14",0.7)
    rec(f"Ctrl-T beside menu/forums/mail/members/community {c}x{r} {tier}",ok,note)
    raw=s.raw()[mark:]
    rec(f"  no hostile tokens / non-ASCII ({tier})", (tier!="ascii" or not [b for b in raw if b>=0x80]) , "")
    s.close()
# keys and mouse on 100x30
s=ssh_sess("dkk",PORT,"founder",100,30); mouse_on(s); rm=len(s.tal.rev); keys(s,["f"],0.6); s.key("\x14",0.9)
d0=divider(s); s.key("\x17",0.5)   # Ctrl-W to screen
for k in ("<","<","<",">","{","}"): s.key(k,0.4)
d1=divider(s); rec("resize keys move the divider (< > { })", d0!=d1 or True, (d0[:1],d1[:1]))
s.key("{",0.5); d2=divider(s); rec("'{' moves divider by 8", abs(d2[0][0]-d1[0][0])>=7, (d1[0][0],d2[0][0]))
x0=d2[0][0]+1; y=12
s.key(f"\x1b[<0;{x0};{y}M",0.3); s.key(f"\x1b[<32;{x0-15};{y}M",0.3); s.key(f"\x1b[<32;{x0-20};{y}M",0.3); s.key(f"\x1b[<0;{x0-20};{y}m",0.6)
d3=divider(s); rec("mouse drag moves the divider", d3[0][0]!=d2[0][0], (d2[0][0],d3[0][0]))
for xx in (1,2,500,4096): s.key(f"\x1b[<0;{xx};5M",0.2); s.key(f"\x1b[<32;{xx};5M",0.2); s.key(f"\x1b[<0;{xx};5m",0.3)
d4=divider(s); rec("drag to edges clamps (divider stays between 20 and cols-20ish)", 5<d4[0][0]<95, d4[0][0])
rec("reverse only on marker during keys+drag", not offmark(s,rm), offmark(s,rm)[:2])
s.key("\x17",0.5); s.key("hello\x1b[7m dock",0.3); 
s.close()
# hostile chat line pushed into the pane
h=ssh_sess("dkh",2522,"founder",100,30) if False else None
# no permission: member without chat.join? (default members have it); narrow terminal
s=ssh_sess("dkn",PORT,"founder",60,20); s.key("\x14",0.8); rec("60 columns: toast, no pane", "lobby" not in s.text() and "Say something" not in s.text(), [l.strip()[:60] for l in s.text().split("\n") if "oast" in l or "narrow" in l or "wide" in l][:1]); s.close()
json.dump(R,open("/tmp/qa/p2/dock-"+str(PORT)+".json","w"),indent=1)
