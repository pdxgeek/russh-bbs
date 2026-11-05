import sys, time
from pathlib import Path
sys.path.insert(0, "/home/user/russh-bbs/docs/qa/ui-6-screens")
import walk
from walk import capture, keys, text, tmux
out = Path("/home/user/russh-bbs/docs/qa/ui-6-screens")
work = Path("/tmp/claude-0/walk6")
# member (session m)
capture("m", out, "20-member-menu-100x30")
keys("m", "a", "o", wait=0.8)       # files is hidden; sysop tools is not theirs
capture("m", out, "21-member-hidden-keys-do-nothing-100x30")
keys("m", "t", wait=0.8)
capture("m", out, "22-member-group-100x30")
keys("m", "Escape", wait=0.5)
keys("m", "r", wait=1)
capture("m", out, "23-member-page-rules-100x30")
keys("m", "b", wait=0.5)
# sysop: new ssh as founder
walk.ssh_as("sy2", work, (100, 30), "id", "founder")
time.sleep(1.5)
keys("sy2", "x", wait=1)
keys("sy2", "o", wait=0.8)
capture("sy2", out, "30-sysop-tools-100x30")
keys("sy2", "m", wait=1.5)
capture("sy2", out, "31-menus-member-role-100x30")
keys("sy2", "Down", "Down", "Down", wait=0.4)
keys("sy2", "t", wait=0.8)
capture("sy2", out, "32-menus-pages-100x30")
keys("sy2", "Enter", wait=1)
capture("sy2", out, "33-menus-page-preview-100x30")
keys("sy2", "b", wait=0.5)
# hostile file, then reload
(work/"menus.toml").write_text('version = 1\n[[menu]]\nid = "main"\n[[menu.item]]\ntarget = "main.mail"\nkey = "x"\nlabel = "\\u001b[31mred"\n')
keys("sy2", "l", wait=1.5)
capture("sy2", out, "34-menus-hostile-refused-100x30")
# restore and reload
(work/"menus.toml").write_text(walk.MENUS)
keys("sy2", "l", wait=1.5)
capture("sy2", out, "35-menus-reloaded-100x30")
