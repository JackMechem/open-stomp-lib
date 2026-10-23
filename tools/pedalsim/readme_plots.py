"""Frequency-response graphs for the README and the docs site, as recorded from the pedal:
  NAM: the real model, the official app's conversion and open-stomp 0.4's and 0.5's;
  IR:  the full IR file in software, the official app's upload and open-stomp 0.4's and 0.5's.

All from one re-amp session (readme session: the reamp_run.py test signal, one block at a time, the rest off):
  DRY / DRY2           every block off (the pedal's dry path)
  A, F, O              AMP = the Ampeg SVT "Gain 10 Ultra Lo and Hi MD 421" model: official, 0.4, 0.5
  C57A/F/O, CNEA/F/O   CAB = "Ampeg 8x10 57 AH" / "Ampeg SVT Neumann": official, 0.4, 0.5
The real model is run in software on the DRY take raised 5.81 dB (the model's input relative to the pedal's dry USB
output), then the pedal's DC blocker, so it hears exactly what the AMP block heard. A CAB response is the CAB run's
sweep response minus the DRY run's.

Every curve is level-matched to the reference over 100 Hz - 8 kHz, so the graphs compare tone only. Import
level is handled separately (NAM normalisation, the IR gain rule); the offsets that were removed are printed.

usage: readme_plots.py <session dir>   (extract from the recordings, then plot)
       readme_plots.py --cached        (plot from results/*.json, no recordings needed)
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
KEYS = ("official", "0.4", "0.5")
NAM_RUNS = dict(zip(KEYS, ("A", "F", "O")))
# (title, file, official, 0.4, 0.5 run)
IRS = (("Ampeg 8x10 57 AH", "Ampeg 8x10 57 AH.wav", "C57A", "C57F", "C57O"),
       ("Ampeg SVT Neumann", "Ampeg SVT Neumann.wav", "CNEA", "CNEF", "CNEO"))
BANDS = 31.5 * 2 ** (np.arange(28) / 3)
SR = 48000
DRY_TO_MODEL_DB = 5.81
DCB = ([0.9953827, -1.990765, 0.9953827], [1, -1.990744, 0.9907867])


def bands_db(H2, f):
    return np.array([10 * np.log10(np.mean(H2[(f >= b / 2 ** (1 / 6)) & (f < b * 2 ** (1 / 6))]) + 1e-30) for b in BANDS])


def third_octaves(h, n=1 << 16):
    H = np.abs(np.fft.rfft(h, n)) ** 2; f = np.fft.rfftfreq(n, 1 / SR)
    return bands_db(H, f)


def extract(session):
    from scipy.signal import lfilter
    sys.path.insert(0, str(HERE))
    from paths import SVT, IRS as IR_DIR
    from open_stomp import nam, ir
    m = json.load(open(session / "map.json")); segs = {s["name"]: s for s in m["segments"]}
    played = np.load(session / "played.npy").astype(np.float64)
    T, f1, f2 = m["sweep_T"], m["f1"], m["f2"]
    t = np.arange(int(T * SR)) / SR; Lw = T / np.log(f2 / f1)
    sw = np.sin(2 * np.pi * f1 * Lw * (np.exp(t / Lw) - 1)); N = 1 << 20
    INV = np.fft.rfft(sw[::-1] * np.exp(-t / Lw), N)

    def timeline(r):
        """r shifted so the played signal's timeline starts at 0 (cross-correlation on the first sweep)."""
        s = segs["sweep -36"]; a = played[s["start"]:s["start"] + SR]; b = r[:s["start"] + 3 * SR]
        n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
        c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)[:3 * SR]
        off = int(np.argmax(np.abs(c)))
        return np.concatenate([r[off:], np.zeros(SR)])[:len(played) + SR]

    def sweep(y, name):
        s = segs[name]; x = y[max(0, s["start"] - 200): s["start"] + s["len"] + 2400]
        h = np.fft.irfft(np.fft.rfft(x, N) * INV, N)
        k = int(np.argmax(np.abs(h))); lin = h[k - 2000:k + 16000] * np.hanning(18000)
        H = np.abs(np.fft.rfft(lin, 1 << 16)) ** 2
        return bands_db(H, np.fft.rfftfreq(1 << 16, 1 / SR)) - int(name.split()[1])

    def tones(y):
        out = {}
        for name, s in segs.items():
            if not name.startswith("tone"): continue
            f = float(name.split()[1]); x = y[s["start"] + 9600: s["start"] + s["len"] - 4800]
            X = np.abs(np.fft.rfft(x * np.hanning(len(x)))); fr = np.fft.rfftfreq(len(x), 1 / SR)
            out[name] = [float(20 * np.log10(X[np.argmin(abs(fr - f * k))] + 1e-12)) for k in (1, 3, 5)]
        return out

    rec = {p.stem: timeline(np.load(p).astype(np.float64)) for p in session.glob("*.npy") if p.stem != "played"}
    dry = rec["DRY"]
    model = nam.Model.load(SVT)
    x = (dry * 10 ** (DRY_TO_MODEL_DB / 20)).astype(np.float32)
    real = lfilter(*DCB, np.array(model.process(x.tolist()), np.float64))
    data = {"bands": BANDS.tolist(), "model": Path(SVT).stem,
            "note": "1/3-octave response from log sweeps, dB, recorded from the pedal's USB output in one session. "
                    "'real' is the model run in software on the pedal's dry take at the model's input level.",
            "real": {k: sweep(real, k).tolist() for k in LEVELS},
            "tones": {"real": tones(real)}}
    for key, run in NAM_RUNS.items():
        data[key] = {k: sweep(rec[run], k).tolist() for k in LEVELS}
        data["tones"][key] = tones(rec[run])
    d = np.array(sweep(rec["DRY"], "sweep -24")) - np.array(sweep(rec["DRY2"], "sweep -24"))
    data["rig_drift_db"] = float(np.sqrt(np.mean(d[(BANDS >= 40) & (BANDS <= 12000)] ** 2)))
    NAM_CACHE.parent.mkdir(exist_ok=True)
    json.dump(data, open(NAM_CACHE, "w"), indent=1)

    dry_resp = {k: np.array(sweep(rec["DRY"], k)) for k in ("sweep -24", "sweep -12")}
    data = {"bands": BANDS.tolist(),
            "note": "1/3-octave response, dB. 'real' is the full IR file at 48 kHz in software; the uploads are the "
                    "pedal's CAB block measured by sweep (-24 dBFS), minus the dry path, in one session.",
            "irs": {}}
    for title, wav, *runs in IRS:
        rate, ch = ir.load_wav(str(Path(IR_DIR) / wav))
        data["irs"][title] = {"real": third_octaves(np.array(ir.resample(ch[0], rate, SR), float)).tolist()}
        for key, run in zip(KEYS, runs):
            data["irs"][title][key] = (np.array(sweep(rec[run], "sweep -24")) - dry_resp["sweep -24"]).tolist()
    json.dump(data, open(IR_CACHE, "w"), indent=1)


if "--cached" not in sys.argv:
    extract(Path(sys.argv[1]))

# light surface, recessive axes; the reference = neutral ink, then categorical slots 1 and 2
SURFACE, INK, MUTED, GRID = "#fcfcfb", "#0b0b0b", "#52514e", "#e4e3df"
REF = "#2b2b2a"
CONVERSIONS = [("official", "Official app", "#eb6834"), ("0.4", "open-stomp 0.4", "#1baf7a"), ("0.5", "open-stomp 0.5", "#2a78d6")]
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
    """panels: [(panel title, {"real": .., "official": .., "0.4": .., "0.5": ..})]"""
    rows, cols = shape
    fig, axes = plt.subplots(rows, cols, figsize=(10, 3.3 * rows + 0.6), sharex=True, sharey=True,
                             facecolor=SURFACE, squeeze=False)
    for ax, (name, curves) in zip(axes.flat, panels):
        style(ax, name)
        ref = np.array(curves["real"])
        if diff:
            ax.axhline(0, color=MUTED, lw=1.2, ls=(0, (4, 3)))   # the reference minus itself
        else:
            ax.plot(bands, ref, color=REF, lw=2.6, solid_capstyle="round")
        for key, label, color in CONVERSIONS:
            y, shift = matched(bands, np.array(curves[key]), ref)
            if diff: print(f"  {path.stem} | {name} | {label}: level shifted {shift:+.1f} dB, "
                           f"rms error {np.sqrt(np.mean((y - ref) ** 2)):.2f} dB")
            ax.plot(bands, y - ref if diff else y, color=color, lw=2.0, solid_capstyle="round")
    for ax in axes[-1]: ax.set_xlabel("Frequency (Hz)")
    for ax in axes[:, 0]: ax.set_ylabel("Difference (dB)" if diff else "Level (dB, matched)")
    handles = [plt.Line2D([], [], color=MUTED, lw=1.2, ls=(0, (4, 3)), label=f"0 dB = same as the {ref_label[0].lower() + ref_label[1:]}")
               if diff else plt.Line2D([], [], color=REF, lw=2.6, label=ref_label)] + \
              [plt.Line2D([], [], color=c, lw=2.0, label=l) for _, l, c in CONVERSIONS]
    h = 3.3 * rows + 0.6   # header in inches from the top: title, legend, then the panels
    fig.text(0.06, 1 - 0.1 / h, title, ha="left", va="top", fontsize=12.5, color=INK, weight="bold")
    fig.legend(handles=handles, loc="upper left", bbox_to_anchor=(0.06, 1 - 0.4 / h), ncol=4, frameon=False)
    fig.tight_layout(rect=(0, 0, 1, 1 - 0.6 / h))
    fig.savefig(path, dpi=150, facecolor=SURFACE); plt.close(fig)
    print("wrote", path)


OUT.mkdir(parents=True, exist_ok=True)
nam = json.load(open(NAM_CACHE))
nb = np.array(nam["bands"])
panels = [(f"Sweep at {k.split()[1]} dBFS", {s: nam[s][k] for s in ("real",) + KEYS}) for k in LEVELS]
figure(panels, nb, OUT / "nam-frequency-response.png",
       "NAM on the pedal: Ampeg SVT bass model (levels matched)", "Real NAM model", False, (2, 2))
figure(panels, nb, OUT / "nam-frequency-error.png",
       "NAM: how far each upload is from the real model (closer to 0 dB is better)", "Real NAM model", True, (2, 2))

irs = json.load(open(IR_CACHE))
ib = np.array(irs["bands"])
panels = list(irs["irs"].items())
figure(panels, ib, OUT / "ir-frequency-response.png",
       "IR on the pedal: Ampeg SVT cab IRs (levels matched)", "Full IR file", False, (1, 2))
figure(panels, ib, OUT / "ir-frequency-error.png",
       "IR: how far each upload is from the full IR file (closer to 0 dB is better)", "Full IR file", True, (1, 2))


# scores (rms dB against the real model / full IR; lower is better), levels matched per curve as in the graphs
def rms(v): return float(np.sqrt(np.mean(np.square(v))))
band = (nb >= 40) & (nb <= 10000); low = (nb >= 40) & (nb <= 160)
scores = {}
for key in KEYS:
    d = [matched(nb, np.array(nam[key][k]), np.array(nam["real"][k]))[0] - np.array(nam["real"][k]) for k in LEVELS]
    T, R = nam["tones"][key], nam["tones"]["real"]
    fund = np.array([T[n][0] - R[n][0] for n in R]); fund -= fund.mean()
    harm = np.array([[(T[n][j] - T[n][0]) - (R[n][j] - R[n][0]) for j in (1, 2)] for n in R])   # relative to the fundamental
    scores[key] = {"frequency response": rms(np.concatenate([x[band] for x in d])),
                   "low end": rms(np.concatenate([x[low] for x in d])),
                   "note dynamics": rms(fund), "harmonics": rms(harm)}
print("\nNAM, rms dB vs the real model:", " ".join(f"{h:>20}" for h in scores["0.5"]))
for key, s in scores.items(): print(f"  {key:10}" + " ".join(f"{v:20.2f}" for v in s.values()))
for title, curves in irs["irs"].items():
    ref = np.array(curves["real"]); m = (ib >= 31.5) & (ib <= 16000)
    for key in KEYS:
        y, shift = matched(ib, np.array(curves[key]), ref)
        print(f"IR {title:18} {key:9} rms {rms((y - ref)[m]):5.2f} dB, 40-160 Hz {rms((y - ref)[(ib >= 40) & (ib <= 160)]):5.2f} dB, level {-shift:+6.1f} dB")
if "rig_drift_db" in nam: print(f"rig drift (DRY vs DRY2, -24 dBFS sweep, 40 Hz-12 kHz): {nam['rig_drift_db']:.2f} dB rms")
