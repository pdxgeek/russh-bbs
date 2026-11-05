from lib import *
import shutil, sys
shutil.rmtree(OUT, ignore_errors=True)
def sess(user, cols, rows, style, layout, first=False):
    t=term(user, cols, rows)
    if first:
        for k in "xny": t.send(k, quiet=0.7)
    settle(t,"Main menu")
    if style: set_choice(t,"Theme",style,"t")
    if layout: set_choice(t,"Layout",layout,"l")
    settle(t,"Main menu"); return t

# ---- bob and carol: first sign-in answers (Unicode), then they write ----
bob=sess("bob",100,30,"neon","dashboard", first=True)
carol=sess("carol",100,30,"neon","dashboard", first=True)
# bob mails the founder, carol posts a one-liner and a wall line
bob.send("m",quiet=0.8); bob.send("c",quiet=0.6); line(bob,"founder"); line(bob,"Welcome aboard, and a question")
bob.send("Hi founder!\rIs the retro night on?\r> you wrote earlier about plans\rThanks.",quiet=0.5); bob.send(SEND,quiet=1.2)
at_menu(bob)
carol.send("y",quiet=0.8); carol.send("o",quiet=0.8); carol.send("p",quiet=0.5); line(carol,"first!")
carol.send("p",quiet=0.5); line(carol,"ratatui looks great on 80 columns")
carol.send("\x1b",quiet=0.6); carol.send("g",quiet=0.8); carol.send("p",quiet=0.5); line(carol,"carol was here"); carol.send("p",quiet=0.5); line(carol,"bob too")
at_menu(carol)

# ---- founder at 132x43 in neon / dashboard ----
f=sess("founder",132,43,"neon","dashboard")
snap(f,"01-dashboard-neon-132x43")
f.send("f",quiet=1.0); snap(f,"02-boards-neon-132x43")
f.send("1",quiet=1.0); snap(f,"03-threads-neon-132x43")
f.send("o",quiet=0.8); snap(f,"04-threads-sorted-by-subject-132x43")
f.send("o",quiet=0.8); f.send("o",quiet=0.8)
# make a long thread to show tabs: open "Welcome to the board" and reply a few times
f.send("1",quiet=1.0)
for _ in range(4):
    f.send("c",quiet=0.9); f.send("a reply to the first post",quiet=0.5); f.send(SEND,quiet=1.2)
snap(f,"05-reader-tabs-quotes-132x43")
f.send("n",quiet=0.8); snap(f,"06-reader-next-post-132x43")
f.send("\x1b",quiet=0.8); f.send("\x1b",quiet=0.8); f.send("\x1b",quiet=0.8); at_menu(f)

# chat: founder and bob in the lobby
bob.send("c",quiet=0.8); bob.send("\r",quiet=1.0)
carol.send("c",quiet=0.8); carol.send("\r",quiet=1.0)
f.send("c",quiet=1.0); snap(f,"07-chat-room-list-132x43"); f.send("\r",quiet=1.0)
for who,text in [(bob,"hello founder! is anyone up for a retro night?"),(carol,"count me in. this looks great on a narrow terminal"),(f,"nice, three panes now: rooms, talk and who is here")]:
    line(who,text,quiet=0.7)
bob.send("/w founder psst, bring snacks\r",quiet=0.8)
f.pump(1.0); f.send("a message being typed", quiet=0.6)
snap(f,"08-chat-three-panes-132x43")
f.send("\x15"*0,quiet=0.1)
for _ in range(30): f.send("\x7f",quiet=0.01)
f.send("\x1b",quiet=0.8); f.send("\x1b",quiet=0.8); at_menu(f)
bob.send("\x1b",quiet=0.6); bob.send("\x1b",quiet=0.6); at_menu(bob)

# mail
f.send("m",quiet=1.0); snap(f,"09-mail-inbox-132x43")
f.send("\r",quiet=1.0); snap(f,"10-mail-reader-132x43")
f.send("\x1b",quiet=0.8); f.send("\x1b",quiet=0.8); at_menu(f)

# community + members
f.send("y",quiet=0.8); f.send("o",quiet=0.9); snap(f,"11-oneliners-132x43"); f.send("\x1b",quiet=0.6)
f.send("g",quiet=0.9); snap(f,"12-wall-132x43"); f.send("\x1b",quiet=0.6)
f.send("l",quiet=0.9); snap(f,"13-last-callers-132x43"); f.send("\x1b",quiet=0.6)
f.send("p",quiet=0.9)
f.send("c",quiet=0.6); line(f,"Best colour for a terminal?"); line(f,"Green"); line(f,"Amber"); line(f,"Blue"); f.send("\r",quiet=0.6); 
snap(f,"14-poll-form-132x43")
f.send("\r",quiet=1.0); snap(f,"15-polls-132x43")
f.send("1",quiet=1.0); f.send("2",quiet=1.0); snap(f,"16-poll-with-bars-132x43")
f.send("\x1b",quiet=0.6); f.send("\x1b",quiet=0.6); at_menu(f)
f.send("u",quiet=1.2); snap(f,"17-members-list-132x43"); f.send("o",quiet=0.6); f.send("\x1b[B",quiet=0.6); snap(f,"18-members-sorted-132x43")
f.send("\r",quiet=1.0); snap(f,"19-profile-132x43")
f.send("\x1b",quiet=0.8); f.send("\x1b",quiet=0.8); at_menu(f)

# split layout with live chat and mail panes
set_choice(f,"Layout","split","l"); f.pump(2.0); snap(f,"20-split-chat-and-mail-panes-132x43")
set_choice(f,"Theme","phosphor","t"); f.pump(1.5); snap(f,"21-split-phosphor-132x43")
set_choice(f,"Layout","wide","l"); set_choice(f,"Theme","paper","t"); f.pump(1.5); snap(f,"22-wide-paper-132x43")
set_choice(f,"Layout","classic","l"); set_choice(f,"Theme","amber","t"); f.send("f",quiet=1.0); f.send("1",quiet=1.0); f.send("1",quiet=1.0); snap(f,"23-reader-classic-amber-132x43")
f.close(); bob.close(); carol.close()

# ---- 80x24 and 100x30, three styles ----
for (cols,rows,style,layout) in [(80,24,"neon","dashboard"),(80,24,"phosphor","classic"),(100,30,"paper","wide")]:
    t=sess("founder",cols,rows,style,layout)
    tag=f"{style}-{layout}-{cols}x{rows}"
    snap(t,f"30-home-{tag}")
    t.send("f",quiet=1.0); t.send("1",quiet=1.0); snap(t,f"31-threads-{tag}")
    t.send("1",quiet=1.0); snap(t,f"32-reader-{tag}")
    t.send("\x1b",quiet=0.8); t.send("\x1b",quiet=0.8); t.send("\x1b",quiet=0.8); at_menu(t)
    t.send("c",quiet=1.0); t.send("\r",quiet=1.0); snap(t,f"33-chat-{tag}"); t.send("\x1b",quiet=0.6); t.send("\x1b",quiet=0.6); at_menu(t)
    t.send("m",quiet=1.0); snap(t,f"34-mail-{tag}"); t.send("\x1b",quiet=0.6); at_menu(t)
    t.send("y",quiet=0.8); t.send("p",quiet=1.0); snap(t,f"35-polls-{tag}")
    t.close()
print("done")
