"""Frequency-response graphs for the README: the real NAM model, the official app's conversion and
open-stomp's optimised conversion, as recorded from the pedal.

usage: readme_plots.py            (re-extract from the local recordings, then plot)
       readme_plots.py --cached   (plot from results/svt_g10_responses.json, no recordings needed)
"""
import json, sys
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

HERE = Path(__file__).resolve().parent
LIB = HERE.parents[1]
CACHE = HERE / "results" / "svt_g10_responses.json"
OUT = LIB / "docs" / "images"
LEVELS = ("sweep -36", "sweep -24", "sweep -12", "sweep -6")

if "--cached" not in sys.argv:
    sys.path.insert(0, str(HERE))
    from paths import D
    F = json.load(open(D + "reamp/svt_g10/nam_features.json"))
    rec = json.load(open(D + "reamp/svt_g10_rust/features.json"))   # recorded in one session
    off = F["offset_db"]
    data = {"bands": F["bands"],
            "model": "Ampeg SVT - Gain 10 Ultra Lo and Hi MD 421 (NAM)",
            "note": "1/3-octave response from log sweeps, dB; recorded from the pedal's USB output. "
                    "'real' is the model run in software at the pedal's level.",
            "real": {k: (np.array(F["nam"][k]) + off).tolist() for k in LEVELS},
            "official": {k: rec["A1"][k] for k in LEVELS},
            "open-stomp": {k: rec["R1"][k] for k in LEVELS}}
    CACHE.parent.mkdir(exist_ok=True)
    json.dump(data, open(CACHE, "w"), indent=1)
data = json.load(open(CACHE))
bands = np.array(data["bands"])

# light surface, recessive axes; real model = neutral reference, then categorical slots 1 and 2
SURFACE, INK, MUTED, GRID = "#fcfcfb", "#0b0b0b", "#52514e", "#e4e3df"
SERIES = [("real", "Real NAM model", "#2b2b2a", 2.6),
          ("official", "Official app", "#eb6834", 2.0),
          ("open-stomp", "open-stomp (optimised)", "#2a78d6", 2.0)]
plt.rcParams.update({"font.family": "sans-serif", "font.size": 10, "text.color": INK, "axes.labelcolor": MUTED,
                     "xtick.color": MUTED, "ytick.color": MUTED, "axes.edgecolor": GRID})

def style(ax, title):
    ax.set_facecolor(SURFACE); ax.set_xscale("log"); ax.set_xlim(30, 17000)
    ax.set_xticks([50, 100, 200, 500, 1000, 2000, 5000, 10000])
    ax.set_xticklabels(["50", "100", "200", "500", "1k", "2k", "5k", "10k"])
    ax.minorticks_off(); ax.grid(True, color=GRID, lw=0.8); ax.set_axisbelow(True)
    for s in ("top", "right"): ax.spines[s].set_visible(False)
    ax.set_title(title, loc="left", fontsize=10.5, color=INK)

def figure(diff, path, title, ylabel):
    fig, axes = plt.subplots(2, 2, figsize=(10, 6.6), sharex=True, sharey=True, facecolor=SURFACE)
    for ax, k in zip(axes.flat, LEVELS):
        style(ax, f"Input {k.split()[1]} dBFS")
        if diff:
            ax.axhline(0, color="#2b2b2a", lw=2.6)
        for key, label, color, lw in SERIES:
            y = np.array(data[key][k])
            if diff:
                if key == "real": continue
                y = y - np.array(data["real"][k])
            ax.plot(bands, y, color=color, lw=lw, label=label, solid_capstyle="round")
    for ax in axes[1]: ax.set_xlabel("Frequency (Hz)")
    for ax in axes[:, 0]: ax.set_ylabel(ylabel)
    if diff:
        handles = [plt.Line2D([], [], color="#2b2b2a", lw=2.6, label="Real NAM model (0 dB)")] + \
                  [plt.Line2D([], [], color=c, lw=lw, label=l) for key, l, c, lw in SERIES if key != "real"]
    else:
        handles = [plt.Line2D([], [], color=c, lw=lw, label=l) for _, l, c, lw in SERIES]
    fig.legend(handles=handles, loc="upper left", bbox_to_anchor=(0.06, 0.95), ncol=3, frameon=False)
    fig.suptitle(title, x=0.06, y=0.985, ha="left", fontsize=12.5, color=INK, weight="bold")
    fig.tight_layout(rect=(0, 0, 1, 0.925))
    fig.savefig(path, dpi=150, facecolor=SURFACE); plt.close(fig)
    print("wrote", path)

OUT.mkdir(parents=True, exist_ok=True)
figure(False, OUT / "nam-frequency-response.png",
       "Frequency response on the pedal: Ampeg SVT bass model", "Output level (dB)")
figure(True, OUT / "nam-frequency-error.png",
       "Difference from the real model (closer to 0 dB is better)", "Difference (dB)")
