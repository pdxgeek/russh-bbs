import sys,re,time,json; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
PORT=2722; R={}
def conn(name,c,r): return Sess(name,f"ssh -tt -i /tmp/qa/keys/founder -p {PORT} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o SetEnv=COLORTERM=truecolor founder@127.0.0.1",c,r,{"TERM":"xterm-256color"},outdir="/tmp/qa/raw")
def setm(want):
    s=ssh_sess("mo",PORT,"founder",80,24,tier="nerd"); s.key("s",0.5)
    if f"Motion: {want}" not in s.text(): s.key("m",0.5)
    s.close()
def cellsig(s,skip="Last login"):
    out=[]
    for y in range(s.rows):
        line="".join(s.scr.buffer[y][x].data for x in range(s.cols))
        if skip in line: continue
        out.append(tuple((s.scr.buffer[y][x].data,s.scr.buffer[y][x].fg,s.scr.buffer[y][x].bg,s.scr.buffer[y][x].bold,s.scr.buffer[y][x].reverse) for x in range(s.cols)))
    return out
def frames(s,secs):
    seen=set(); ch=0; t0=time.time()
    while time.time()-t0<secs:
        time.sleep(0.1); s.feed(); h=hash(tuple(cellsig(s,"@@")))
        if h not in seen: seen.add(h)
    return len(seen)
for (c,r) in ((80,24),(132,43)):
    setm("on"); s=conn("i",c,r); n=frames(s,4.5); st=sgr_stats(s.raw()); on=cellsig(s); R[f"intro {c}x{r} on"]=dict(bytes=len(s.raw()),distinct_frames=n,gfx=st["gfx"],rev=s.tal.summary()); s.close()
    setm("off"); s=conn("o",c,r); n=frames(s,3.0); off=cellsig(s); R[f"login {c}x{r} motion off"]=dict(bytes=len(s.raw()),distinct_frames=n,equals_intro_final=(on==off)); s.close()
    # skip
    setm("on"); s=conn("k",c,r); time.sleep(0.5); s.key(" ",0.6); s.key("s",0.6); R[f"skip {c}x{r}"]=dict(menu_then_settings=s.has("Width:")); s.close()
# UI-4 screens: bytes and distinct frames after entering each screen, motion on vs off, neon style
for motion in ("on","off"):
    s=ssh_sess("u4"+motion,PORT,"founder",100,30,tier="nerd"); s.key("s",0.5)
    for _ in range(12):
        if "Theme: neon" in s.text(): break
        s.key("t",0.3)
    if f"Motion: {motion}" not in s.text(): s.key("m",0.5)
    s.key("\x1b",0.8)
    for nm,sq in (("forums",["f"]),("thread list",["f","1"]),("reader",["f","1","1"]),("mail",["m"]),("chat",["c"]),("community",["y"]),("members",["u"])):
        keys(s,["ESC"]*5,0.3); s.settle(0.5); mark=len(s.raw())
        for k in sq: subprocess.run(["tmux","send-keys","-t",s.name,"-l",k])
        seen=set(); t0=time.time()
        while time.time()-t0<2.5:
            time.sleep(0.05); s.feed(); seen.add(hash(tuple(cellsig(s,"@@"))))
        raw=s.raw()[mark:]
        R[f"{nm if False else nm} motion {motion}"]=dict(bytes=len(raw),distinct_frames=len(seen),gfx=sgr_stats(raw)["gfx"])
    R[f"UI-4 rev motion {motion}"]=[x[:3] for x in s.tal.rev if not (x[0]==29 and x[3])][:2]
    s.close()
setm("on")
json.dump(R,open("/tmp/qa/p2/fx3.json","w"),indent=1,default=str)
for k,v in R.items(): print(k,v)
