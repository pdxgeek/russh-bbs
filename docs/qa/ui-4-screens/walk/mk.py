from lib import *
for h in ["bob","carol"]:
    t=join(h); line(t,h); pw="a throwaway walk password"; line(t,pw); line(t,pw, quiet=1.5); t.close()
print("members made")
