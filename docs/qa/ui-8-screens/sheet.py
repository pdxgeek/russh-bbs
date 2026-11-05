#!/usr/bin/env python3
"""Turn the frames `walk.py capture` recorded into contact sheets and a GIF.

    python3 sheet.py CAPTURE_DIR OUT_DIR

Each `NAME/frame-NN.cells` is drawn with `../m4-s3-screens/render.py`. Frames that look the same
as the one before are dropped, then up to eight evenly spaced frames go in `NAME-sheet.png` and
all of them in `NAME.gif`.
"""
import os
import subprocess
import sys

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
RENDER = os.path.join(HERE, "..", "m4-s3-screens", "render.py")


def distinct(folder):
    seen, picked = None, []
    for name in sorted(n for n in os.listdir(folder) if n.endswith(".cells")):
        body = open(os.path.join(folder, name), encoding="utf-8").read()
        if body != seen:
            picked.append(name[:-6])
        seen = body
    return picked


def render(folder, frame, size, tmp):
    png = os.path.join(tmp, f"{os.path.basename(folder)}-{frame}.png")
    cols, rows = size
    subprocess.run(
        [sys.executable, RENDER, os.path.join(folder, frame + ".cells"), png, str(cols), str(rows)],
        check=True,
    )
    return Image.open(png).convert("RGB")


def sheet(name, size, scale_sheet, scale_gif, capture, out, tmp):
    folder = os.path.join(capture, name)
    frames = distinct(folder)
    images = [render(folder, f, size, tmp) for f in frames]
    step = max(1, len(images) // 8)
    chosen = images[::step][:8]
    w, h = (int(chosen[0].width * scale_sheet), int(chosen[0].height * scale_sheet))
    columns = 4
    rows = (len(chosen) + columns - 1) // columns
    canvas = Image.new("RGB", (w * columns, h * rows), (12, 16, 20))
    for i, img in enumerate(chosen):
        canvas.paste(img.resize((w, h), Image.LANCZOS), ((i % columns) * w, (i // columns) * h))
    canvas.save(os.path.join(out, f"{name}-sheet.png"), optimize=True)
    small = [img.resize((int(img.width * scale_gif), int(img.height * scale_gif)), Image.LANCZOS) for img in images]
    small[0].save(
        os.path.join(out, f"{name}.gif"),
        save_all=True,
        append_images=small[1:],
        duration=110,
        loop=0,
        optimize=True,
    )
    print(name, len(frames), "distinct frames")


if __name__ == "__main__":
    capture, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    tmp = os.path.join(out, ".tmp")
    os.makedirs(tmp, exist_ok=True)
    sheet("intro-132x43", (132, 43), 0.30, 0.34, capture, out, tmp)
    sheet("intro-80x24", (80, 24), 0.50, 0.55, capture, out, tmp)
    sheet("intro-ascii-80x24", (80, 24), 0.50, 0.55, capture, out, tmp)
    sheet("interrupt-132x43", (132, 43), 0.30, 0.34, capture, out, tmp)
    for leftover in os.listdir(tmp):
        os.remove(os.path.join(tmp, leftover))
    os.rmdir(tmp)
