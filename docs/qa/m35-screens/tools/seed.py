import sqlite3, subprocess, sys, time
db, kind = sys.argv[1], sys.argv[2]
ESC = "\x1b"
def fp(u): return subprocess.run(["ssh-keygen","-lf",f"/tmp/qa/keys/{u}.pub"],capture_output=True,text=True).stdout.split()[1]
def fit(p, n, filler):  # payload first, then filler, exactly n chars
    return (p + filler * n)[:n]
if kind == "hostile":
    P = lambda tag: f"{tag}{ESC}[7;53m{ESC}]0;QAPWN\x07\x9b97;53m⟦⊖⟧\U0010fffd〚☉〛◉‮​⁦{ESC}[0m\x00\x08\x7f\r\t"
else:
    P = lambda tag: f"{tag}" + "z" * 36
c = sqlite3.connect(db); c.execute("pragma foreign_keys=on")
now = int(time.time())
def acct(name, skel, key=None):
    cur = c.execute("insert into accounts(username, username_skeleton, state, created_at, invited_by) values (?,?,?,?,1)", (name, skel, "active", now)); i = cur.lastrowid
    if key: c.execute("insert into account_keys(fingerprint, account_id, created_at) values (?,?,?)", (fp(key), i, now))
    c.execute("insert into last_caller(account_id,last_at) values (?,?)", (i, now))
    return i
bob = acct("bob", "bob", "bob")
hs = [acct(fit(P(f"H{i}"), 30, "w"), f"hostileskel{i}", f"h{i}") for i in (1, 2, 3)]
def board(name, desc, restricted=0, feed=0):
    return c.execute("insert into board(name,name_key,description,restricted,created_by,created_at,feed) values (?,?,?,?,1,?,?)", (name, name.lower(), desc, restricted, now, feed)).lastrowid
gen = board("General", fit(P("D"), 160, "d"))
hb = board(fit(P("B"), 40, "b"), "hostile board name")
back = board("Backroom", "BACKROOM-CANARY-DESC", 1)
feedb = board("Newswire", "feed board", 0, 1)
sfeed = board("SecretFeed", "SECRETFEED-CANARY-DESC", 1, 1)
def thread(b, subj, body, author, reply=None):
    t = c.execute("insert into thread(board_id,subject,author_id,created_at,last_post_at) values (?,?,?,?,?)", (b, subj, author, now, now)).lastrowid
    c.execute("insert into post(thread_id,board_id,author_id,subject,body,created_at) values (?,?,?,?,?,?)", (t, b, author, subj, body, now))
    if reply: c.execute("insert into post(thread_id,board_id,author_id,subject,body,created_at) values (?,?,?,'',?,?)", (t, b, hs[1], reply, now + 1))
    return t
for i in range(3):
    thread(gen, fit(P(f"S{i}"), 118, "s"), "\n".join(P(f"L{j}") + " " + "word " * 12 for j in range(6)), hs[i], reply=P("R") + " reply " * 20)
thread(hb, fit(P("HB"), 100, "t"), P("body") * 3, hs[0])
thread(feedb, fit(P("FEED"), 120, "f"), P("feedbody") + "\nhttps://example.invalid/" + "x" * 90, hs[2])
thread(back, "BACKROOM-CANARY-SUBJECT", "BACKROOM-CANARY-BODY " + "x", 1)
thread(sfeed, "SECRETFEED-CANARY-SUBJECT", "SECRETFEED-CANARY-BODY", 1)
for i in range(3):
    c.execute("insert into community_line(kind,account_id,body,created_at) values ('oneliner',?,?,?)", (hs[i], fit(P(f"O{i}"), 70, "o"), now + i))
    c.execute("insert into community_line(kind,account_id,body,created_at) values ('graffiti',?,?,?)", (hs[i], fit(P(f"G{i}"), 70, "g"), now + i))
c.execute("insert into community_line(kind,account_id,body,created_at) values ('oneliner',1,'BACKROOM-CANARY-ONELINER-is-fine-founder-wrote-it',?)", (now,))
pid = c.execute("insert into poll(creator_id,question,description,results,created_at) values (?,?,?,'always',?)", (hs[0], fit(P("Q"), 60, "q"), fit(P("PD"), 350, "p"), now)).lastrowid
for i in range(4): c.execute("insert into poll_option(poll_id,position,text) values (?,?,?)", (pid, i, fit(P(f"P{i}"), 40, "o")))
c.execute("insert into chat_room(name,created_at,created_by) values (?,?,?)", (fit(P("CR"), 32, "c"), now, hs[0]))
# restricted board grant for nobody but staff: Backroom stays closed to bob. Grant bob the open boards only (default).
c.commit(); print("seeded", kind)
