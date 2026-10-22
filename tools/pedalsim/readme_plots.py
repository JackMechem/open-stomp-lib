"""Frequency-response graphs for the README, as recorded from the pedal:
  NAM: the real model, the official app's conversion and open-stomp's optimised conversion;
  IR:  the full IR file in software, the official app's upload and open-stomp's optimised upload.

Every curve is level-matched to the reference over 100 Hz - 8 kHz, so the graphs compare tone only. Import
level is handled separately (NAM normalisation, the IR gain rule); the offsets that were removed are printed.

usage: readme_plots.py            (re-extract from the local recordings, then plot)
       readme_plots.py --cached   (plot from results/*.json, no recordings needed)
"""
import json, sys
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

HERE = Path(__file__).resolve().parent
LIB = HERE.parents[1]
NAM_CACHE = HERE / "results" / "svt_g10_responses.json"
IR_CACHE = HERE / "results" / "ir_cab_responses.json"
OUT = LIB / "docs" / "images"
LEVELS = ("sweep -36", "sweep -24", "sweep -12", "sweep -6")
# (title, file, official run, open-stomp run) from reamp/ir_cab
IRS = (("Ampeg 8x10 57 AH", "Ampeg 8x10 57 AH.wav", "A57", "O57"),
       ("Ampeg SVT Neumann", "Ampeg SVT Neumann.wav", "ANEU", "ONEU"))
BANDS = 31.5 * 2 ** (np.arange(28) / 3)


def third_octaves(h, n=1 << 16):
    H = np.abs(np.fft.rfft(h, n)) ** 2; f = np.fft.rfftfreq(n, 1 / 48000)
    return [float(10 * np.log10(np.mean(H[(f >= b / 2 ** (1 / 6)) & (f < b * 2 ** (1 / 6))]) + 1e-30)) for b in BANDS]


if "--cached" not in sys.argv:
    sys.path.insert(0, str(HERE))
    from paths import D, IRS as IR_DIR
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
    NAM_CACHE.parent.mkdir(exist_ok=True)
    json.dump(data, open(NAM_CACHE, "w"), indent=1)

    from open_stomp import ir
    R = Path(D) / "reamp" / "ir_cab"
    data = {"bands": BANDS.tolist(),
            "note": "1/3-octave response, dB. 'real' is the full IR file at 48 kHz in software; the uploads are "
                    "the pedal's CAB block measured by sweep (-24 dBFS), divided by the dry rig path.",
            "irs": {}}
    for title, wav, a, o in IRS:
        rate, ch = ir.load_wav(str(Path(IR_DIR) / wav))
        data["irs"][title] = {"real": third_octaves(np.array(ir.resample(ch[0], rate, 48000), float)),
                              "official": third_octaves(np.load(R / f"rel_{a}_-24.npy")),
                              "open-stomp": third_octaves(np.load(R / f"rel_{o}_-24.npy"))}
    json.dump(data, open(IR_CACHE, "w"), indent=1)

# light surface, recessive axes; the reference = neutral ink, then categorical slots 1 and 2
SURFACE, INK, MUTED, GRID = "#fcfcfb", "#0b0b0b", "#52514e", "#e4e3df"
REF = "#2b2b2a"
CONVERSIONS = [("official", "Official app", "#eb6834"), ("open-stomp", "open-stomp (optimised)", "#2a78d6")]
plt.rcParams.update({"font.family": "sans-serif", "font.size": 10, "text.color": INK, "axes.labelcolor": MUTED,
                     "xtick.color": MUTED, "ytick.color": MUTED, "axes.edgecolor": GRID})


def matched(bands, y, ref):
    """y shifted so its mean over 100 Hz - 8 kHz equals the reference's; and the shift."""
    m = (bands >= 100) & (bands <= 8000)
    shift = float(np.mean(ref[m] - y[m]))
    return y + shift, shift


def style(ax, title):
    ax.set_facecolor(SURFACE); ax.set_xscale("log"); ax.set_xlim(30, 17000)
    ax.set_xticks([50, 100, 200, 500, 1000, 2000, 5000, 10000])
    ax.set_xticklabels(["50", "100", "200", "500", "1k", "2k", "5k", "10k"])
    ax.minorticks_off(); ax.grid(True, color=GRID, lw=0.8); ax.set_axisbelow(True)
    for s in ("top", "right"): ax.spines[s].set_visible(False)
    ax.set_title(title, loc="left", fontsize=10.5, color=INK)


def figure(panels, bands, path, title, ref_label, diff, shape):
    """panels: [(panel title, {"real": .., "official": .., "open-stomp": ..})]"""
    rows, cols = shape
    fig, axes = plt.subplots(rows, cols, figsize=(10, 3.3 * rows + 0.6), sharex=True, sharey=True,
                             facecolor=SURFACE, squeeze=False)
    for ax, (name, curves) in zip(axes.flat, panels):
        style(ax, name)
        ref = np.array(curves["real"])
        if diff:
            ax.axhline(0, color=REF, lw=2.6)
        else:
            ax.plot(bands, ref, color=REF, lw=2.6, solid_capstyle="round")
        for key, label, color in CONVERSIONS:
            y, shift = matched(bands, np.array(curves[key]), ref)
            if diff: print(f"  {path.stem} | {name} | {label}: level shifted {shift:+.1f} dB, "
                           f"rms error {np.sqrt(np.mean((y - ref) ** 2)):.2f} dB")
            ax.plot(bands, y - ref if diff else y, color=color, lw=2.0, solid_capstyle="round")
    for ax in axes[-1]: ax.set_xlabel("Frequency (Hz)")
    for ax in axes[:, 0]: ax.set_ylabel("Difference (dB)" if diff else "Level (dB, matched)")
    handles = [plt.Line2D([], [], color=REF, lw=2.6, label=ref_label + (" (0 dB)" if diff else ""))] + \
              [plt.Line2D([], [], color=c, lw=2.0, label=l) for _, l, c in CONVERSIONS]
    h = 3.3 * rows + 0.6   # header in inches from the top: title, legend, then the panels
    fig.text(0.06, 1 - 0.1 / h, title, ha="left", va="top", fontsize=12.5, color=INK, weight="bold")
    fig.legend(handles=handles, loc="upper left", bbox_to_anchor=(0.06, 1 - 0.4 / h), ncol=3, frameon=False)
    fig.tight_layout(rect=(0, 0, 1, 1 - 0.6 / h))
    fig.savefig(path, dpi=150, facecolor=SURFACE); plt.close(fig)
    print("wrote", path)


OUT.mkdir(parents=True, exist_ok=True)
nam = json.load(open(NAM_CACHE))
nb = np.array(nam["bands"])
panels = [(f"Input {k.split()[1]} dBFS", {s: nam[s][k] for s in ("real", "official", "open-stomp")}) for k in LEVELS]
figure(panels, nb, OUT / "nam-frequency-response.png",
       "NAM on the pedal: Ampeg SVT bass model (levels matched)", "Real NAM model", False, (2, 2))
figure(panels, nb, OUT / "nam-frequency-error.png",
       "NAM: difference from the real model (closer to 0 dB is better)", "Real NAM model", True, (2, 2))

irs = json.load(open(IR_CACHE))
ib = np.array(irs["bands"])
panels = list(irs["irs"].items())
figure(panels, ib, OUT / "ir-frequency-response.png",
       "IR on the pedal: Ampeg SVT cab IRs (levels matched)", "Full IR file", False, (1, 2))
figure(panels, ib, OUT / "ir-frequency-error.png",
       "IR: difference from the full IR file (closer to 0 dB is better)", "Full IR file", True, (1, 2))
