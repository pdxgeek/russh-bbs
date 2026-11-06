#!/usr/bin/env python3
"""Walk the file areas over a real OpenSSH client (`ssh` and `sftp`) and capture the Files screens.

    python3 docs/qa/m4-f2-screens/walk_files.py WORKDIR OUTDIR [blobs-dir-or-s3]

Starts the real server binary on a fresh board (`target/debug/russh-bbs-server`, SQLite, local
blobs), becomes the founder with `ssh claim@host`, seeds three more members straight into the
database (ann, mo the moderator, bob; the same way the earlier QA walks did), then:

1. the founder makes an area from the Files screen;
2. ann uploads two files with `sftp put` (one larger than one packet);
3. bob lists the area with `sftp` and cannot see them, and cannot read them by id;
4. mo opens Files, the approval queue and the file, and approves one and rejects the other;
5. bob lists, downloads the approved file with `sftp get`, and the hashes are compared;
6. bob opens Files in the TUI: area, file with its sftp path, adds it to the download list.

Each stop is written as NAME.txt (what `pyte` shows) and NAME.png (drawn by tools/screenshots).
Keys are generated here with `ssh-keygen` and live in WORKDIR; nothing is committed. Needs
`pip install --user pyte`, Pillow, OpenSSH `ssh`, `ssh-keygen` and `sftp`.
"""
import hashlib
import os
import re
import sqlite3
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "docs", "qa", "m4-s3-screens"))
sys.path.insert(0, os.path.join(ROOT, "tools", "screenshots"))
import walk as base  # noqa: E402  (Term and claim, shared with the other walks)
import shot  # noqa: E402

PORT = 2299
USERS = ["ann", "mo", "bob"]


def sh(*args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw)


def make_keys(work):
    keys = {}
    for name in ["founder"] + USERS:
        path = f"{work}/{name}"
        if not os.path.exists(path):
            sh("ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", path)
        keys[name] = path
    return keys


def fingerprint(path):
    return sh("ssh-keygen", "-lf", path + ".pub").stdout.split()[1]


def start_server(work):
    env = dict(
        os.environ,
        RUSSH_BBS_LISTEN=f"127.0.0.1:{PORT}",
        RUSSH_BBS_HOST_KEY=f"{work}/hostkey",
        RUSSH_BBS_DB=f"{work}/bbs.sqlite",
        RUSSH_BBS_BLOBS_DIR=f"{work}/blobs",
        RUSSH_BBS_RATE_LIMITS="off",
    )
    log = open(f"{work}/server.log", "wb")
    proc = subprocess.Popen([f"{ROOT}/target/debug/russh-bbs-server"], env=env, stdout=log, stderr=log)
    token = None
    for _ in range(100):
        time.sleep(0.2)
        text = open(f"{work}/server.log", errors="replace").read()
        m = re.search(r"([A-Z0-9]{4}(?:-[A-Z0-9]{4}){7})", text)
        if m:
            token = m.group(1)
            break
    return proc, token


def seed(work, keys):
    db = sqlite3.connect(f"{work}/bbs.sqlite")
    db.execute("pragma foreign_keys=on")
    now = int(time.time())
    for name in USERS:
        cur = db.execute(
            "insert into accounts(username, username_skeleton, state, created_at, invited_by) values (?,?,?,?,1)",
            (name, name, "active", now),
        )
        acct = cur.lastrowid
        db.execute(
            "insert into account_keys(fingerprint, account_id, created_at) values (?,?,?)",
            (fingerprint(keys[name]), acct, now),
        )
        if name == "mo":
            db.execute("insert into access_grant(account_id, role) values (?, 'moderator')", (acct,))
    db.commit()
    db.close()


def sftp(work, keys, who, *commands):
    batch = f"{work}/{who}-batch.txt"
    open(batch, "w").write("\n".join(commands) + "\n")
    p = subprocess.run(
        ["sftp", "-b", batch, "-P", str(PORT), "-i", keys[who],
         "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
         "-o", "IdentitiesOnly=yes", "-o", "LogLevel=ERROR", f"{who}@127.0.0.1"],
        capture_output=True, text=True, cwd=work,
    )
    return p.returncode, p.stdout + p.stderr


def tui(keys, who, cols=80, rows=24):
    term = base.Term(PORT, keys[who], cols, rows, user=who)
    term.pump(2.5)
    for _ in range(4):
        if term.has("Icon check") or term.has("icon"):
            term.send("y", quiet=1.0)
        elif term.has("Any key") or term.has("press any key"):
            term.send(" ", quiet=1.0)
        else:
            break
    return term


def snap(term, out, name):
    base_name = f"{out}/{name}"
    os.makedirs(out, exist_ok=True)
    open(base_name + ".txt", "w", encoding="utf-8").write(term.text() + "\n")
    shot.render_pyte(term.screen, base_name + ".png", scale=1)
    print("captured", name)


def main():
    work, out = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
    os.makedirs(work, exist_ok=True)
    keys = make_keys(work)
    report = []
    proc, token = start_server(work)
    try:
        assert token, "no setup token in the server log"
        base.claim(PORT, keys["founder"], token)
        seed(work, keys)

        # 1. the founder makes an area from the Files screen
        term = tui(keys, "founder")
        term.send("a", quiet=1.0)
        snap(term, out, "01-areas-empty-founder")
        term.send("n", quiet=0.5)
        term.send("Manuals", quiet=0.5)
        term.send("\r", quiet=0.5)
        snap(term, out, "02-new-area-form")
        term.send("\r", quiet=1.2)
        snap(term, out, "03-areas-with-manuals")
        term.close()

        # 2. ann uploads over sftp
        small = f"{work}/router-guide.txt"
        big = f"{work}/network-diagram.bin"
        open(small, "wb").write(b"How to reset the router.\n" * 40)
        open(big, "wb").write(os.urandom(700_000))
        code, text = sftp(work, keys, "ann", "ls -1 /", "put router-guide.txt /1-manuals/router-guide.txt",
                          "put network-diagram.bin /1-manuals/network-diagram.bin", "ls -l /1-manuals/pending")
        report.append(("ann: sftp upload", code, text))

        # 3. bob cannot see them
        code, text = sftp(work, keys, "bob", "ls -l /1-manuals", "ls -l /1-manuals/pending",
                          "get /1-manuals/pending/1-router-guide.txt stolen.txt")
        report.append(("bob before approval (the get must fail)", code, text))
        assert not os.path.exists(f"{work}/stolen.txt")

        # 4. mo reviews and decides
        term = tui(keys, "mo")
        term.send("a", quiet=1.0)
        snap(term, out, "04-areas-moderator")
        term.send("q", quiet=1.0)
        snap(term, out, "05-queue")
        term.send("1", quiet=1.0)
        snap(term, out, "06-file-awaiting-approval")
        term.send("a", quiet=1.5)
        snap(term, out, "07-after-approve")
        term.send("1", quiet=1.0)
        term.send("r", quiet=0.6)
        snap(term, out, "08-confirm-reject")
        term.send("y", quiet=1.5)
        snap(term, out, "09-after-reject")
        term.close()

        # 5. bob downloads
        code, text = sftp(work, keys, "bob", "ls -l /1-manuals", "get /1-manuals/1-router-guide.txt got.txt")
        report.append(("bob after approval", code, text))
        same = hashlib.sha256(open(small, "rb").read()).hexdigest() == hashlib.sha256(open(f"{work}/got.txt", "rb").read()).hexdigest()
        report.append(("bob's download equals the upload (sha256)", 0 if same else 1, str(same)))

        # 6. bob in the TUI, at 80x24 and 132x43
        for cols, rows, tag in [(80, 24, "80x24"), (132, 43, "132x43")]:
            term = tui(keys, "bob", cols, rows)
            term.send("a", quiet=1.0)
            snap(term, out, f"10-bob-areas-{tag}")
            term.send("1", quiet=1.0)
            snap(term, out, f"11-bob-area-{tag}")
            term.send("1", quiet=1.0)
            snap(term, out, f"12-bob-file-{tag}")
            term.send("b", quiet=1.0)
            term.send("\x1b", quiet=0.6)
            term.send("l", quiet=1.0)
            snap(term, out, f"13-bob-download-list-{tag}")
            term.close()

        code, text = sftp(work, keys, "bob", "ls -l /downloads", "get /downloads/1-router-guide.txt listed.txt",
                          "mkdir /1-manuals/x", "rm /1-manuals/1-router-guide.txt", "get /../../etc/passwd pw.txt")
        report.append(("bob: the download list, then things that must be refused", code, text))
    finally:
        proc.terminate()
        proc.wait(timeout=10)
    with open(f"{out}/sftp-transcripts.txt", "w") as f:
        for title, code, text in report:
            f.write(f"=== {title} (exit {code})\n{text}\n")
    print(open(f"{out}/sftp-transcripts.txt").read())


if __name__ == "__main__":
    main()
