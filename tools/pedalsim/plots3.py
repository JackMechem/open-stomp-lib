import json, sys
from pathlib import Path
import numpy as np
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FixedLocator, FuncFormatter, NullLocator
D = Path(sys.argv[1]); OUT = D / "plots"
j = json.load(open(D / "nam_features.json")); B = np.array(j["bands"]); off = j["offset_db"]; N = j["nam"]; R = j["rec"]
SURFACE, TEXT, TEXT2, MUTED, GRID = "#fcfcfb", "#0b0b0b", "#52514e", "#8a8984", "#e6e5e0"
NAMC, OFFICIAL, OURS, CLEAN = "#1baf7a", "#2a78d6", "#eb6834", "#4a3aa7"
plt.rcParams.update({"figure.facecolor": SURFACE, "axes.facecolor": SURFACE, "savefig.facecolor": SURFACE,
    "axes.edgecolor": GRID, "axes.labelcolor": TEXT2, "xtick.color": TEXT2, "ytick.color": TEXT2, "text.color": TEXT,
    "axes.grid": True, "grid.color": GRID, "grid.linewidth": 0.8, "axes.spines.top": False, "axes.spines.right": False,
    "font.size": 10.5, "axes.titlesize": 11.5, "axes.titleweight": "bold", "axes.titlelocation": "left",
    "legend.frameon": False, "lines.linewidth": 2})
SER = [("nam", NAMC, "Real NAM model (software)"), ("A1", OFFICIAL, "Official conversion"), ("B1", OURS, "Open, app mode"), ("C1", CLEAN, "Open, clean mode")]
fig, axes = plt.subplots(2, 2, figsize=(12.5, 8.6), sharex=True, sharey=True)
for ax, lv in zip(axes.flat, (-36, -24, -12, -6)):
    k = f"sweep {lv}"; n1k = R["A1"][k][int(np.argmin(abs(B - 1000)))]
    for src, col, lab in SER:
        v = (np.array(N[k]) + off) if src == "nam" else np.array(R[src][k])
        ax.plot(B, v - n1k, color=col, label=lab, marker="o" if src == "nam" else None, markersize=3.5)
    ax.set_xscale("log"); ax.set_xlim(28, 17000)
    ax.xaxis.set_major_locator(FixedLocator([31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000])); ax.xaxis.set_minor_locator(NullLocator())
    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v/1000:g}k" if v >= 1000 else f"{v:g}"))
    ax.set_title(f"Input {lv} dBFS"); ax.label_outer()
for ax in axes[1]: ax.set_xlabel("Frequency (Hz)")
for ax in axes[:, 0]: ax.set_ylabel("Level re official at 1 kHz (dB)")
axes[0, 0].legend(loc="lower left", fontsize=9)
fig.suptitle("Three conversions on the pedal vs the real model (Ampeg SVT G10 MD 421)", x=0.01, ha="left", fontsize=12.5, fontweight="bold")
fig.text(0.01, 0.005, "Re-amped through the pedal, AMP block only. 1/3-octave linear response; one calibration shared by all.", color=MUTED, fontsize=8.5)
fig.tight_layout(rect=(0, 0.025, 1, 0.97)); fig.savefig(OUT / "8_three_conversions_response.png", dpi=150); plt.close(fig)

notes = [41.2, 55.0, 73.4, 98.0, 196.0]; names = ["E1 41", "A1 55", "D2 73", "G2 98", "G3 196"]
fig, axes = plt.subplots(1, 3, figsize=(14, 4.8), sharey=True)
x = np.arange(len(notes)); offs = (-0.21, -0.07, 0.07, 0.21)
for ax, lv in zip(axes, (-24, -12, -6)):
    for (src, col, lab), o in zip(SER, offs):
        h3 = [N[f"tone {f} {lv}"][1] if src == "nam" else R[src][f"tone {f} {lv}"][1] for f in notes]
        ax.scatter(x + o, h3, s=55, color=col, edgecolor=SURFACE, linewidth=1.5, label=lab, zorder=3)
    ax.set_xticks(x, [f"{n}" for n in names]); ax.grid(axis="x", visible=False); ax.set_title(f"3rd harmonic, input {lv} dBFS")
    ax.set_xlabel("Note (Hz)")
axes[0].set_ylabel("dB re fundamental"); axes[0].legend(loc="lower left", fontsize=9)
fig.text(0.01, 0.005, "Held sine tones. Higher = more distortion.", color=MUTED, fontsize=8.5)
fig.tight_layout(rect=(0, 0.03, 1, 1)); fig.savefig(OUT / "9_three_conversions_distortion.png", dpi=150); plt.close(fig)
print("ok")
