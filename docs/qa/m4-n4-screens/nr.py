"""Helpers for the Nullroute social walk (reuses the terminal helper of the S3 walk)."""
import os, re, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m4-s3-screens"))
from walk import Term  # noqa: E402

def open_door(term):
    term.pump(2.0)
    if term.has("Any key"):
        term.send(" ")
    if term.has("Icon check"):
        term.send("y")
    term.send("d", quiet=1.0)
    number = None
    for line in term.text().splitlines():
        m = re.search(r"\[(\d+)\] Nullroute", line)
        if m:
            number = m.group(1)
    if number is None:
        print(term.text()); raise SystemExit("no Nullroute entry")
    term.send(number, quiet=1.2)

def keys(term, seq, quiet=0.6):
    """Send keys; tokens split on spaces; 'enter','esc','down','up' are named."""
    names = {"enter": "\r", "esc": "\x1b", "down": "\x1b[B", "up": "\x1b[A", "sp": " "}
    for tok in seq.split(" "):
        term.send(names.get(tok, tok), quiet=quiet)
