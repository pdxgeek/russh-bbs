from lib import *
import json
def rev_cells(t):
    out=[]
    for y in range(t.rows):
        row=t.screen.buffer[y]
        for x in range(t.cols):
            c=row[x]
            if c.reverse: out.append((y,x,c.bold,c.fg,c.bg))
    return out
res={}
for (cols,rows) in [(80,24),(132,43)]:
    t=term("founder",cols,rows); 
    for k in "xny": t.send(k,quiet=0.8)
    settle(t,"Main menu"); t.pump(2.0)
    for label,keys in [("main menu",""),("forums","f"),("mail","m"),("chat","c"),("community polls","yp"),("members","u"),("settings","s")]:
        at_menu(t)
        for k in keys: t.send(k,quiet=0.9)
        t.pump(0.8)
        cells=rev_cells(t); last=rows-1
        off=[c for c in cells if c[0]!=last]; inrow=[c for c in cells if c[0]==last]
        ok=all(c[2] for c in cells) and all(str(c[3]) in("default",) and str(c[4]) in("default",) for c in cells)
        res[f"{cols}x{rows} {label}"]=dict(rev_cells_in_status_row=len(inrow),rev_cells_elsewhere=len(off),all_bold_no_colour=ok,first_marker_text=t.screen.display[last][:40].strip())
        print(cols,rows,label,res[f"{cols}x{rows} {label}"])
        for _ in range(3): t.send("\x1b",quiet=0.5)
    t.close()
json.dump(res,open(OUT+"-marker.json","w"),indent=1)
