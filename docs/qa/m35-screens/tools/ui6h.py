import sys,re,json,os,shutil; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
D="/tmp/qa/d2/M"; good=open(D+"/menus.toml").read(); R={}
def W(txt, name="menus.toml"): open(f"{D}/{name}","w").write(txt)
def menu_of(user):
    s=ssh_sess("mh"+user,3322,user,100,30); t=[l.strip(" │")[:20] for l in s.text().split("\n") if re.search(r"\[\w\]",l)][:4]; s.close(); return t
f=ssh_sess("mhf",3322,"founder",100,30); keys(f,["o","m"],0.8)
base=menu_of("bob")
def variant(name, txt, fname="menus.toml"):
    W(txt, fname); f.key("l",1.2); t=f.text(); line=[l.strip(" │")[:90] for l in t.split("\n") if re.search(r"(?i)refus|reload|loaded|not valid|bad|unchanged|Problem|Notice",l)][:2]
    after=menu_of("bob")
    leak = bool(re.search(r"\x1b|\ufffd.*\ufffd.*\ufffd", t)) 
    R[name]=dict(response=line, member_menu_unchanged=(after==base), screen_has_marker=[(y,x) for y,x,c,b in f.revcells() if b][:1])
    print(f"{name:34s} {line} | bob menu unchanged: {after==base}", flush=True)
    W(good); f.key("l",1.0)
hdr='version = 1\n[[menu]]\nid = "main"\n  [[menu.item]]\n  target = "main.mail"\n'
variant("ESC in label", hdr+'  key = "x"\n  label = "Hi\\u001b[1;7mX"\n')
variant("marker glyphs in label", hdr+'  key = "x"\n  label = "\\u27e6\\u2296\\u27e7 mail"\n')
variant("NerdFont PUA in label", hdr+'  key = "x"\n  label = "mail \\uf132"\n')
variant("privacy claim in label", hdr+'  key = "x"\n  label = "Private mail"\n')
variant("unknown field", hdr+'  key = "x"\n  action = "shell"\n')
variant("duplicate key", hdr+'  key = "f"\n  label = "Dup"\n')
variant("string-table key as label", hdr+'  key = "x"\n  label = "menu.main.mail.item"\n')
variant("wide/zero-width label", hdr+'  key = "x"\n  label = "\\u200b\\u202e日本語日本語日本語日本語日本語日本語日本語日本語日本語"\n')
variant("target a made-up area", 'version = 1\n[[menu]]\nid = "main"\n  [[menu.item]]\n  target = "files.upload"\n  key = "u"\n')
variant("cycle group", 'version = 1\n[[menu]]\nid = "main"\n  [[menu.item]]\n  target = "group:a"\n  key = "a"\n[[menu]]\nid = "a"\n  [[menu.item]]\n  target = "group:a"\n  key = "b"\n')
variant("700 lines", 'version = 1\n'+"# x\n"*700)
variant("version 2", 'version = 2\n')
variant("page missing", 'version = 1\n[[menu]]\nid = "main"\n  [[menu.item]]\n  target = "page:nope"\n  key = "n"\n')
variant("page art traversal", good, "menus.toml"); 
open(D+"/pages/rules.toml","w").write('version = 1\ntitle = "House rules"\nart = "../../../../etc/passwd"\ntext = "x"\n'); f.key("l",1.2); print("page art traversal:", [l.strip(" │")[:90] for l in f.text().split("\n") if re.search(r"(?i)refus|not valid|bad|Problem",l)][:2], "menu unchanged:", menu_of("bob")==base)
open(D+"/pages/rules.toml","w").write('version = 1\ntitle = "House rules"\ntext = "x\\u001b[1;7m\\u27e6\\u2296\\u27e7\\uf132\\u202e hostile\\u0007 \\u200b zero"\n'); f.key("l",1.2); print("page hostile text:", [l.strip(" │")[:90] for l in f.text().split("\n") if re.search(r"(?i)refus|not valid|bad|Problem",l)][:2])
f.close()
