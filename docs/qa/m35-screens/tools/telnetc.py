#!/usr/bin/env python3
"""Raw telnet client for the QA walk: answers NAWS and TTYPE, passes everything else through.
usage: telnetc.py HOST PORT COLS ROWS TERMTYPE RAWLOG"""
import os, select, socket, sys, termios, tty
host, port, cols, rows, ttype, rawlog = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), sys.argv[5], sys.argv[6]
IAC, DONT, DO, WONT, WILL, SB, SE = 255, 254, 253, 252, 251, 250, 240
NAWS, TTYPE, SGA, ECHO = 31, 24, 3, 1
s = socket.create_connection((host, port))
log = open(rawlog, "wb")
tty.setraw(0)
out = sys.stdout.buffer
def naws(): return bytes([IAC, SB, NAWS, cols >> 8, cols & 255, rows >> 8, rows & 255, IAC, SE])
buf = b""
def feed(data):
    global buf
    buf += data; res = b""; i = 0
    while i < len(buf):
        b = buf[i]
        if b != IAC:
            res += bytes([b]); i += 1; continue
        if i + 1 >= len(buf): break
        c = buf[i + 1]
        if c == IAC: res += b"\xff"; i += 2
        elif c in (DO, DONT, WILL, WONT):
            if i + 2 >= len(buf): break
            o = buf[i + 2]
            if c == DO and o == NAWS: s.sendall(bytes([IAC, WILL, NAWS]) + naws())
            elif c == DO and o == TTYPE: s.sendall(bytes([IAC, WILL, TTYPE]))
            elif c == DO and o == SGA: s.sendall(bytes([IAC, WILL, SGA]))
            elif c == WILL and o in (ECHO, SGA): s.sendall(bytes([IAC, DO, o]))
            elif c == DO: s.sendall(bytes([IAC, WONT, o]))
            elif c == WILL: s.sendall(bytes([IAC, DONT, o]))
            i += 3
        elif c == SB:
            j = buf.find(bytes([IAC, SE]), i)
            if j < 0: break
            if buf[i + 2] == TTYPE and buf[i + 3] == 1:
                s.sendall(bytes([IAC, SB, TTYPE, 0]) + ttype.encode() + bytes([IAC, SE]))
            i = j + 2
        else: i += 2
    buf = buf[i:]
    return res
try:
    while True:
        r, _, _ = select.select([0, s], [], [])
        if s in r:
            d = s.recv(65536)
            if not d: break
            log.write(d); log.flush()
            out.write(feed(d)); out.flush()
        if 0 in r:
            d = os.read(0, 4096)
            if not d: break
            s.sendall(d.replace(b"\xff", b"\xff\xff"))
finally:
    pass
