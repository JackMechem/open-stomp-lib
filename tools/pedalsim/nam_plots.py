import json, sys
from pathlib import Path
import numpy as np
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FixedLocator, FuncFormatter, NullLocator
D = Path(sys.argv[1]); OUT = D / "plots"; MODEL = sys.argv[2]
j = json.load(open(D / "nam_features.json")); B = np.array(j["bands"]); off = j["offset_db"]; N = j["nam"]; R = j["rec"]
SURFACE, TEXT, TEXT2, MUTED, GRID = "#fcfcfb", "#0b0b0b", "#52514e", "#8a8984", "#e6e5e0"
OFFICIAL, OURS, NAMC = "#2a78d6", "#eb6834", "#1baf7a"
plt.rcParams.update({"figure.facecolor": SURFACE, "axes.facecolor": SURFACE, "savefig.facecolor": SURFACE,
    "axes.edgecolor": GRID, "axes.labelcolor": TEXT2, "xtick.color": TEXT2, "ytick.color": TEXT2, "text.color": TEXT,
    "axes.grid": True, "grid.color": GRID, "grid.linewidth": 0.8, "axes.spines.top": False, "axes.spines.right": False,
    "font.size": 10.5, "axes.titlesize": 11.5, "axes.titleweight": "bold", "axes.titlelocation": "left",
    "legend.frameon": False, "lines.linewidth": 2})
def cap(fig, t): fig.text(0.01, 0.005, t, color=MUTED, fontsize=8.5)

fig, axes = plt.subplots(1, 2, figsize=(12.5, 5.2), sharey=True)
for ax, lv in zip(axes, (-24, -6)):
    k = f"sweep {lv}"; ref = np.array(R["A1"][k]); n1k = ref[np.argmin(abs(B - 1000))]
    ax.plot(B, np.array(N[k]) + off - n1k, color=NAMC, label="Real NAM model (software)", marker="o", markersize=4)
    ax.plot(B, ref - n1k, color=OFFICIAL, label="Official conversion (pedal)")
    ax.plot(B, np.array(R["B1"][k]) - n1k, color=OURS, label="Open conversion (pedal)")
    ax.set_xscale("log"); ax.set_xlim(28, 17000)
    ax.xaxis.set_major_locator(FixedLocator([31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000])); ax.xaxis.set_minor_locator(NullLocator())
    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v/1000:g}k" if v >= 1000 else f"{v:g}"))
    ax.set_title(f"Input {lv} dBFS"); ax.set_xlabel("Frequency (Hz)")
axes[0].set_ylabel("Level re official at 1 kHz (dB)"); axes[0].legend(loc="lower left", fontsize=9.5)
fig.suptitle(f"Pedal conversions vs the real model: {MODEL}", x=0.01, ha="left", fontsize=12.5, fontweight="bold")
cap(fig, "1/3-octave linear response from the sweeps. Model rendered with this crate's NAM engine; one input/output calibration shared by all.")
fig.tight_layout(rect=(0, 0.035, 1, 0.97)); fig.savefig(OUT / "6_vs_real_model_response.png", dpi=150); plt.close(fig)

notes = [41.2, 55.0, 73.4, 98.0, 196.0]; names = ["E1 41", "A1 55", "D2 73", "G2 98", "G3 196"]
fig, axes = plt.subplots(1, 2, figsize=(12.5, 5.2))
x = np.arange(len(notes)); offs = (-0.15, 0, 0.15)
for (src, col, lab), o in zip(((None, NAMC, "Real NAM model"), ("A1", OFFICIAL, "Official (pedal)"), ("B1", OURS, "Open (pedal)")), offs):
    fund = [(N[f"tone {f} -12"][0] + off) if src is None else R[src][f"tone {f} -12"][0] for f in notes]
    h3 = [N[f"tone {f} -12"][1] if src is None else R[src][f"tone {f} -12"][1] for f in notes]
    axes[0].scatter(x + o, fund, s=65, color=col, edgecolor=SURFACE, linewidth=2, label=lab, zorder=3)
    axes[1].scatter(x + o, h3, s=65, color=col, edgecolor=SURFACE, linewidth=2, label=lab, zorder=3)
for ax in axes:
    ax.set_xticks(x, [f"{n} Hz" for n in names]); ax.grid(axis="x", visible=False)
axes[0].set_ylabel("Fundamental level (dB, calibrated)"); axes[0].set_title("Note level, input −12 dBFS")
axes[1].set_ylabel("3rd harmonic re fundamental (dB)"); axes[1].set_title("Distortion, input −12 dBFS")
axes[0].legend(loc="lower right", fontsize=9.5)
cap(fig, "Held sine tones on the open strings. Below ~70 Hz the real model cuts the fundamental hard; both pedal conversions keep it.")
fig.tight_layout(rect=(0, 0.035, 1, 1)); fig.savefig(OUT / "7_vs_real_model_notes.png", dpi=150); plt.close(fig)
print("ok")
