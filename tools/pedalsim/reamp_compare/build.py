"""Build the interactive re-amp comparison page from analyze.py's data.

usage: build.py <data.json> <out.html> [--audio BASE EXT NOTE] [--back HREF LABEL]

  --audio  publish the A/B player: the takes are at BASE + key + EXT (e.g. "audio/" ".opus"); NOTE is shown under it.
           Without it the Listen card is hidden and only the analysis is shown.
  --back   a link back to the page that embeds this one (the docs site).
"""
import json
import sys
from pathlib import Path

args = sys.argv[1:]
data, out = args[0], args[1]
D = json.loads(Path(data).read_text())
D["audio"] = None
if "--audio" in args:
    i = args.index("--audio")
    D["audio"] = {"base": args[i + 1], "ext": args[i + 2], "note": args[i + 3]}
if "--back" in args:
    i = args.index("--back")
    D["back"] = {"href": args[i + 1], "label": args[i + 2]}
t = (Path(__file__).parent / "template.html").read_text()
d = json.dumps(D, separators=(",", ":"))
assert "</" not in d
Path(out).write_text(t.replace("/*DATA*/", d))
print("built", out)
