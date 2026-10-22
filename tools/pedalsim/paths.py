"""Where the analysis data lives. Nothing here is in git: the re-amp recordings, the official app's
converter dumps and the third-party model stay local.

PEDALSIM_DATA   directory with `reamp/` (recordings, fits, results) and `namconv/` (official converter
                dumps from open-stomp-manager `tools/namconv`); default: a sibling open-stomp-manager
                checkout's `analysis/extracted/`
PEDALSIM_MODEL  the .nam model the analysis was done on (Ampeg SVT "Gain 10 Ultra Lo and Hi MD 421")
"""
import os
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
D = os.environ.get("PEDALSIM_DATA", str(_ROOT.parent / "open-stomp-manager" / "analysis" / "extracted")).rstrip("/") + "/"
R = D + "reamp/svt_g10/"
SVT = os.environ.get("PEDALSIM_MODEL", os.path.expanduser(
    "~/Desktop/Ampeg SVT Classic with 6x10/Ampeg SVT - Gain 10 Ultra Lo and Hi MD 421.nam"))
