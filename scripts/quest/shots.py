import subprocess, sys, os
from PIL import Image
n, tag = int(sys.argv[1]), sys.argv[2]
d = os.environ.get("QUEST_SERIAL", "2G0YC1ZG1003WN")
out = os.path.join(os.path.dirname(__file__), "..", "..", ".local", "quest")
res = []
for i in range(1, n + 1):
    f = os.path.abspath(os.path.join(out, f"{tag}_{i}.png"))
    subprocess.run(["npx", "-y", "metavr", "capture", "screenshot", "-d", d, "-o", f], capture_output=True, shell=True, timeout=100)
    mx = Image.open(f).convert("L").getextrema()[1]
    res.append(mx)
    print(tag, i, "max", mx, flush=True)
print("black frames:", sum(1 for m in res if m < 20), "/", n)
