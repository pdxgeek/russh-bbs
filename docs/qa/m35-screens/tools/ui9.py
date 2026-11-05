import sys, re, json, time; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
PORT=int(sys.argv[1]); R={}
def rec(k,ok,note=""): R[k]=(bool(ok),str(note)); print(("PASS " if ok else "FAIL "),k,"|",note,flush=True)
def mouse(s, b, x, y, up=False): s.key(f"\x1b[<{b};{x};{y}{'m' if up else 'M'}", 0.6)
def rowof(s, pat):
    for y,l in enumerate(s.text().split("\n")):
        m=re.search(pat,l)
        if m: return y+1, m.start()+1
    return None
def setmouse(s, want):
    s.key("s",0.5)
    if f"Mouse: {want}" not in s.text(): s.key("o",0.6)
    s.key("\x1b",0.7)
# --- mouse off: reports are dropped, not typed
s=ssh_sess("m0",PORT,"bob",80,24); setmouse(s,"off")
r0=rowof(s,r"\[m\] Mail"); mouse(s,0,r0[1]+3,r0[0]); mouse(s,0,r0[1]+3,r0[0],True)
rec("mouse off: click on '[m] Mail' ignored (still on main menu)", s.has("Main menu") and not s.has("mailbox"), r0)
# --- mouse on: click on a menu row presses its hotkey
setmouse(s,"on"); r=rowof(s,r"\[m\] Mail")
mouse(s,0,r[1]+3,r[0]); mouse(s,0,r[1]+3,r[0],True)
rec("mouse on: click on '[m] Mail' opens Mail", s.has("Mail") and (s.has("messages") or s.has("mailbox") or s.has("Your mailbox")), r)
keys(s,["ESC"],0.6)
# wheel in a list
keys(s,["f"],0.6); mouse(s,65,20,8); mouse(s,64,20,8)
rec("wheel in forum board list does not crash", s.has("Forums") or s.has("General"), "")
keys(s,["ESC"],0.5)
# hostile mouse reports
bad=["\x1b[<99999999;1;1M","\x1b[<0;99999;1M","\x1b[<0;1;"+"9"*400+"M","\x1b[<0;1;1;1;1M","\x1b[<-1;5;5M","\x1b[M\xff\xff\xff","\x1b[<0;1;1"+"x"*200,"\x1b[<64;1;1M"*70,"\x1b[<4;5;5M","\x1b[<32;5;5M","\x1b[<8;5;5M"]
for b in bad: s.key(b,0.15)
s.settle(1.0)
alive=subprocess.run(["tmux","list-panes","-t","m0","-F","#{pane_dead}"],capture_output=True,text=True).stdout.strip()=="0"
rec("hostile mouse reports: session alive, menu intact", alive and s.has("Main menu"), "")
# nothing from the reports became text: open one-liner add field and see
keys(s,["y","o","p"],0.6)
fld="\n".join(s.text().split("\n")[5:12])
rec("junk reports typed nothing into a text field", "<" not in fld and "99999" not in fld and "xxxx" not in fld, "")
keys(s,["ESC"]*4,0.3)
rev=[(y,x) for y,x,c,b in s.tal.rev if not (y==23 and b)]
rec("reverse only on marker cell during all mouse use", not rev, rev[:3])
s.close()
# --- palette
for user,exp in (("bob",False),("founder",True)):
    s=ssh_sess("pl"+user,PORT,user,80,24)
    res={}
    for q in ("Backroom","SecretFeed","Sysop","Queue","Hello","Doors","Gen"):
        s.key("\x0b",0.7)
        for ch in q: s.key(ch,0.12)
        s.settle(0.4); t=s.text(); res[q]=("Nothing matches" not in t) and any(q.lower() in l.lower() for l in t.split("\n")[3:20] if "│" in l and "│>" not in l.replace(" ",""))
        s.key("\x1b",0.5)
    rec(f"palette as {user}: restricted boards/sysop entries", (res["Backroom"]==exp and res["SecretFeed"]==exp and res["Sysop"]==exp), res)
    s.close()
# --- toast vs marker
s=ssh_sess("tt",PORT,"founder",80,24); setmouse(s,"off"); s.key("s",0.5); s.key("o",0.3)
t=s.text().split("\n"); rev=[(y,x) for y,x,c,b in s.revcells()]
rec("toast visible and marker cell intact", "Notice: Mouse" in s.text() and rev==[(23,0)], rev[:3])
s.close()
json.dump(R,open("/tmp/qa/p2/ui9.json","w"),indent=1)
