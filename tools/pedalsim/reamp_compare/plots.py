"""Static graph of a re-amp comparison for the docs: each take's difference from the reference by third-octave band,
loudness-matched (analyze.py's band_bias). Same style as readme_plots.py.

usage: plots.py <data.json> <out.png>
"""
import json
import sys

import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

SURFACE, INK, MUTED, GRID = "#fcfcfb", "#0b0b0b", "#52514e", "#e1e0d9"
COLORS = {"new": "#2a78d6", "ori": "#4a3aa7", "old": "#1baf7a", "vendor": "#eb6834"}
plt.rcParams.update({"font.family": "sans-serif", "font.size": 10, "text.color": INK, "axes.labelcolor": MUTED,
                     "xtick.color": MUTED, "ytick.color": MUTED, "axes.edgecolor": GRID})

D = json.load(open(sys.argv[1]))
labels = {k: l for k, _, l in D["takes"]}
f = np.array(D["third"])
fig, ax = plt.subplots(figsize=(10, 4.6), facecolor=SURFACE)
ax.set_facecolor(SURFACE); ax.set_xscale("log"); ax.set_xlim(45, 9000)
ticks = [50, 100, 200, 500, 1000, 2000, 5000]
ax.set_xticks(ticks, [f"{t // 1000}k" if t >= 1000 else str(t) for t in ticks])
ax.minorticks_off(); ax.grid(True, color=GRID, lw=0.8); ax.set_axisbelow(True)
for s in ("top", "right"):
    ax.spines[s].set_visible(False)
ax.axhline(0, color=MUTED, lw=1.2, ls=(0, (4, 3)))
ax.axvspan(45, 160, color=MUTED, alpha=0.06, lw=0)
ax.text(47, 0.97, "low end", transform=ax.get_xaxis_transform(), va="top", color=MUTED, fontsize=9)
for k in D["cmp"]:
    y = np.array([np.nan if v is None else v for v in D["band_bias"][k]])
    keep = (f >= 45) & (f <= 9000)
    ax.plot(f[keep], y[keep], color=COLORS[k], lw=2.6 if k == "new" else 1.8, solid_capstyle="round",
            label=f"{labels[k]} ({D['ltas_err'][k]:.1f} dB rms)", zorder=3 if k == "new" else 2)
ax.set_xlabel("Frequency (Hz)"); ax.set_ylabel("Difference (dB)")
ax.legend(loc="lower left", frameon=False, fontsize=9.5, ncols=2)
fig.text(0.06, 0.965, "Re-amped bass DI: how far each version is from the original model + IR (closer to 0 dB is better)",
         ha="left", va="top", fontsize=12, color=INK, weight="bold")
fig.tight_layout(rect=(0, 0, 1, 0.93))
fig.savefig(sys.argv[2], dpi=150, facecolor=SURFACE)
print("wrote", sys.argv[2])
