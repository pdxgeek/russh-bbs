import os, re, subprocess, time, pyte, shutil
Q = os.path.dirname(os.path.abspath(__file__))
def sh(*a, **k): return subprocess.run(a, capture_output=True, text=True, **k).stdout
BR = re.compile(rb"\x1b\[([0-9;]*)m")
class Tally(pyte.Screen):
    """Counts every cell the server drew (or erased) while reverse video was active."""
    def __init__(self, *a):
        super().__init__(*a); self.rev = []; self.drawn = 0
    def draw(self, data):
        self.drawn += len(data)
        a = self.cursor.attrs
        if a.reverse:
            for i, ch in enumerate(data):
                self.rev.append((self.cursor.y, self.cursor.x + i, ch, a.bold))
        super().draw(data)
    def _blank(self, y, x0, x1):
        a = self.cursor.attrs
        if a.reverse:
            for x in range(x0, x1): self.rev.append((y, x, " ", a.bold))
    def erase_in_line(self, how=0, private=False):
        if how == 0: self._blank(self.cursor.y, self.cursor.x, self.columns)
        elif how == 1: self._blank(self.cursor.y, 0, self.cursor.x + 1)
        else: self._blank(self.cursor.y, 0, self.columns)
        super().erase_in_line(how, private)
    def erase_characters(self, count=None):
        self._blank(self.cursor.y, self.cursor.x, min(self.columns, self.cursor.x + (count or 1)))
        super().erase_characters(count)
    def erase_in_display(self, how=0, *a, **k):
        if self.cursor.attrs.reverse: self.rev.append((self.cursor.y, self.cursor.x, "<ED>", self.cursor.attrs.bold))
        super().erase_in_display(how, *a, **k)
    def summary(self):
        rows = {}
        for y, x, ch, b in self.rev:
            rows.setdefault((y, b), set()).add(x)
        return {f"row{y}{'+bold' if b else ''}": (min(xs), max(xs), len(xs)) for (y, b), xs in sorted(rows.items())}
class Sess:
    """A client in a tmux pane, read through pyte; raw bytes tapped by pipe-pane."""
    def __init__(self, name, cmd, cols=80, rows=24, env=None, outdir="/tmp/qa"):
        os.makedirs(outdir, exist_ok=True)
        self.name, self.cols, self.rows = name, cols, rows
        self.rawf = f"{outdir}/{name}.raw"; open(self.rawf, "wb").close()
        subprocess.run(["tmux", "kill-session", "-t", name], capture_output=True)
        e = " ".join(f"{k}={v}" for k, v in (env or {}).items())
        subprocess.run(["tmux", "new-session", "-d", "-s", name, "-x", str(cols), "-y", str(rows), f"env {e} {cmd}; sleep 600"], check=True)
        subprocess.run(["tmux", "pipe-pane", "-O", "-t", name, f"cat >> {self.rawf}"], check=True)
        self.scr = pyte.Screen(cols, rows); self.st = pyte.ByteStream(self.scr); self.pos = 0
        self.tal = Tally(cols, rows); self.tst = pyte.ByteStream(self.tal)
    def resize(self, c, r):
        subprocess.run(["tmux", "resize-window", "-t", self.name, "-x", str(c), "-y", str(r)], check=True)
        self.cols, self.rows = c, r; self.scr.resize(r, c); self.tal.resize(r, c)
    def key(self, k, wait=0.35):
        subprocess.run(["tmux", "send-keys", "-t", self.name, "-l", k] if len(k) > 1 or k.isprintable() else ["tmux", "send-keys", "-t", self.name, k], check=True)
        self.settle(wait)
    def named(self, k, wait=0.35):
        subprocess.run(["tmux", "send-keys", "-t", self.name, k], check=True); self.settle(wait)
    def settle(self, quiet=0.35, limit=8):
        end = time.time() + limit; last = os.path.getsize(self.rawf); t = time.time()
        while time.time() < end:
            time.sleep(0.05); n = os.path.getsize(self.rawf)
            if n != last: last = n; t = time.time()
            elif time.time() - t > quiet: break
        self.feed()
    def feed(self):
        data = open(self.rawf, "rb").read()[self.pos:]; self.pos += len(data)
        for part in re.split(rb"(\x1b\[\?1049[hl])", data):
            if part == b"\x1b[?1049h": self.scr.reset(); self.tal.reset()
            elif part == b"\x1b[?1049l": pass
            else: self.st.feed(part); self.tst.feed(part)
    def text(self):
        return "\n".join("".join(self.scr.buffer[y][x].data for x in range(self.cols)).rstrip() for y in range(self.rows))
    def has(self, s): return s in self.text()
    def revcells(self):
        out = []
        for y in range(self.rows):
            row = self.scr.buffer[y]
            for x in range(self.cols):
                c = row[x]
                if c.reverse: out.append((y, x, c.data, c.bold))
        return out
    def raw(self): return open(self.rawf, "rb").read()
    def close(self):
        subprocess.run(["tmux", "kill-session", "-t", self.name], capture_output=True)
def sgr_stats(raw):
    n_br = n_rev = n_trc = n_256 = 0
    for m in BR.finditer(raw):
        p = [x for x in m.group(1).split(b";") if x != b""]
        if b"7" in p and b"38" not in p[:0]:
            # crude: parameter 7 as its own token outside a 38/48 sub-sequence
            i = 0; toks = []
            while i < len(p):
                if p[i] in (b"38", b"48") and i + 1 < len(p):
                    i += 3 if p[i + 1] == b"5" else 5; continue
                toks.append(p[i]); i += 1
            if b"7" in toks:
                n_rev += 1
                if b"1" in toks: n_br += 1
    n_trc = len(re.findall(rb"\x1b\[[0-9;]*[34]8;2;", raw)); n_256 = len(re.findall(rb"\x1b\[[0-9;]*[34]8;5;", raw))
    gfx = len(re.findall(rb"\x1bP[0-9;]*q|\x1b_G|\x1b\]1337;", raw))
    return dict(bytes=len(raw), reverse_sgr=n_rev, bold_reverse_sgr=n_br, truecolor=n_trc, c256=n_256, gfx=gfx)
