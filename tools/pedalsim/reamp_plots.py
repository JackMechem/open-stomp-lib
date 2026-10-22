"""Charts for the re-amp A/B test (official vs open conversion of one NAM model)."""
import json, sys
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FixedLocator, FuncFormatter, NullLocator

D = Path(sys.argv[1]); OUT = D / "plots"; OUT.mkdir(exist_ok=True)
TITLE_MODEL = sys.argv[2] if len(sys.argv) > 2 else "NAM model"
m = json.load(open(D / "map.json")); SR = m["sr"]
played = np.load(D / "played.npy").astype(np.float64)
runs = {n: np.load(D / f"{n}.npy").astype(np.float64) for n, _ in m["runs"]}
segs = m["segments"]

# palette (dataviz reference instance, light mode; validated)
SURFACE, TEXT, TEXT2, MUTED, GRID = "#fcfcfb", "#0b0b0b", "#52514e", "#8a8984", "#e6e5e0"
OFFICIAL, OURS, REPEAT = "#2a78d6", "#eb6834", "#a3a29c"
RAMP = ["#86b6ef", "#5598e7", "#2a78d6", "#1c5cab"]     # drive levels, light -> dark

plt.rcParams.update({
    "figure.facecolor": SURFACE, "axes.facecolor": SURFACE, "savefig.facecolor": SURFACE,
    "axes.edgecolor": GRID, "axes.labelcolor": TEXT2, "xtick.color": TEXT2, "ytick.color": TEXT2,
    "text.color": TEXT, "axes.grid": True, "grid.color": GRID, "grid.linewidth": 0.8,
    "axes.spines.top": False, "axes.spines.right": False, "font.size": 10.5,
    "axes.titlesize": 12, "axes.titleweight": "semibold", "axes.titlelocation": "left",
    "legend.frameon": False, "lines.linewidth": 2, "lines.solid_capstyle": "round",
})

# ---- analysis helpers (same as reamp_analyze.py) ----
def xcorr_lag(a, b, maxlag):
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    c = np.concatenate([c[-maxlag:], c[:maxlag + 1]])
    i = int(np.argmax(c)); y0, y1, y2 = c[max(i-1, 0)], c[i], c[min(i+1, len(c)-1)]
    den = y0 - 2*y1 + y2
    return i - maxlag + (0.5 * (y0 - y2) / den if den else 0.0)

def shift(x, d):
    X = np.fft.rfft(x); f = np.fft.rfftfreq(len(x))
    return np.fft.irfft(X * np.exp(-2j*np.pi*f*d), len(x))

first = segs[0]
offsets = {n: xcorr_lag(played[first["start"]:first["start"] + SR], r[:first["start"] + 3*SR], 3*SR - 1) for n, r in runs.items()}
PAD = 2000
def seg(name, s):
    o = int(round(offsets[name])) + s["start"] - PAD
    return runs[name][o:o + s["len"] + 2*PAD]

def aligned(name, s, ref="A1"):
    a, b = seg(ref, s), seg(name, s)
    return shift(b, -xcorr_lag(a, b, 400))

T, f1, f2 = m["sweep_T"], m["f1"], m["f2"]
t = np.arange(int(T*SR))/SR; L = T/np.log(f2/f1)
sw = np.sin(2*np.pi*f1*L*(np.exp(t/L)-1)); inv = sw[::-1]*np.exp(-t/L)
def response(x, smooth_oct=1/12):
    n = 1 << 20
    h = np.fft.irfft(np.fft.rfft(x, n) * np.fft.rfft(inv, n), n)
    k = int(np.argmax(np.abs(h))); lin = h[k-2000:k+16000] * np.hanning(18000)
    H = np.abs(np.fft.rfft(lin, 1 << 16))**2; fr = np.fft.rfftfreq(1 << 16, 1/SR)
    fc = np.geomspace(25, 16000, 400)
    out = np.array([10*np.log10(np.mean(H[(fr >= f/2**(smooth_oct/2)) & (fr < f*2**(smooth_oct/2))])) for f in fc])
    return fc, out

def log_axis(ax, lo=25, hi=16000):
    ax.set_xscale("log"); ax.set_xlim(lo, hi)
    ticks = [t for t in (31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000) if lo <= t <= hi]
    ax.xaxis.set_major_locator(FixedLocator(ticks)); ax.xaxis.set_minor_locator(NullLocator())
    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v/1000:g}k" if v >= 1000 else f"{v:g}"))
    ax.set_xlabel("Frequency (Hz)")

def caption(fig, text):
    fig.text(0.01, 0.005, text, color=MUTED, fontsize=8.5, ha="left", va="bottom")

sweeps = [s for s in segs if s["name"].startswith("sweep")]
levels = [int(s["name"].split()[1]) for s in sweeps]
resp = {s["name"]: {n: response(seg(n, s)[PAD-200:PAD+s["len"]+400]) for n in runs} for s in sweeps}
ref_1k = None

# ---- 1. frequency response, official vs ours, small multiples per drive level ----
fig, axes = plt.subplots(2, 2, figsize=(11, 7), sharex=True, sharey=True)
for ax, s, lv in zip(axes.flat, sweeps, levels):
    fc, a1 = resp[s["name"]]["A1"]; _, b1 = resp[s["name"]]["B1"]
    norm = a1[np.argmin(abs(fc - 1000))]
    ax.plot(fc, a1 - norm, color=OFFICIAL, label="Official conversion (USER AMP 06)")
    ax.plot(fc, b1 - norm, color=OURS, label="Open conversion (USER AMP 07)")
    ax.set_title(f"Input {lv} dBFS", fontsize=11)
    log_axis(ax)
    ax.label_outer()
for ax in axes[:, 0]:
    ax.set_ylabel("Level re official at 1 kHz (dB)")
axes[0, 0].legend(loc="lower center", fontsize=9.5)
fig.suptitle(f"Frequency response through the pedal: {TITLE_MODEL}", x=0.01, ha="left", fontsize=13, fontweight="semibold")
caption(fig, "Exponential sine sweeps, linear part (Farina deconvolution), 1/12-octave smoothing. AMP block only, re-amped and recorded over USB.")
fig.tight_layout(rect=(0, 0.03, 1, 0.97)); fig.savefig(OUT / "1_frequency_response.png", dpi=150); plt.close(fig)

# ---- 2. difference ours - official per drive level, vs repeatability ----
fig, ax = plt.subplots(figsize=(11, 5.2))
ax.axhline(0, color=MUTED, linewidth=1)
for s, lv, col in zip(sweeps, levels, RAMP):
    fc, a1 = resp[s["name"]]["A1"]
    b = (resp[s["name"]]["B1"][1] + resp[s["name"]]["B2"][1]) / 2
    ax.plot(fc, b - a1, color=col, label=f"Input {lv} dBFS")
fc, a1 = resp[sweeps[2]["name"]]["A1"]; _, a2 = resp[sweeps[2]["name"]]["A2"]
ax.plot(fc, a2 - a1, color=REPEAT, linewidth=1.5, linestyle=(0, (4, 3)), label="Repeatability (official run 2 − run 1)")
log_axis(ax); ax.set_xlim(22, 16000)
ax.set_ylabel("Open − official (dB)")
ax.set_title("Where the open conversion differs: the low bass")
ax.legend(loc="lower right", fontsize=9.5, ncol=1)
caption(fig, "Average of the two open runs minus the first official run. The grey dashed line is the rig's own run-to-run difference.")
fig.tight_layout(rect=(0, 0.04, 1, 1)); fig.savefig(OUT / "2_response_difference.png", dpi=150); plt.close(fig)

# ---- 3. low notes: fundamental level and 3rd harmonic ----
tones = [s for s in segs if s["name"].startswith("tone")]
freqs = sorted({float(s["name"].split()[1]) for s in tones})
tlevels = sorted({int(s["name"].split()[2]) for s in tones})
names = {41.2: "E1 41 Hz", 55.0: "A1 55 Hz", 73.4: "D2 73 Hz", 98.0: "G2 98 Hz", 196.0: "G3 196 Hz"}
def harmonics(name, s, f):
    x = seg(name, s)[PAD + 9600: PAD + s["len"] - 4800]
    X = np.abs(np.fft.rfft(x*np.hanning(len(x)))); fr = np.fft.rfftfreq(len(x), 1/SR)
    return [20*np.log10(X[np.argmin(abs(fr - f*k))] + 1e-12) for k in (1, 3)]
res = {(float(s["name"].split()[1]), int(s["name"].split()[2])): {n: harmonics(n, s, float(s["name"].split()[1])) for n in ("A1", "B1")} for s in tones}

fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 5.2))
x = np.arange(len(freqs)); w = 0.25
for i, (lv, col) in enumerate(zip(tlevels, RAMP[1:])):
    d = [res[(f, lv)]["B1"][0] - res[(f, lv)]["A1"][0] for f in freqs]
    ax1.bar(x + (i - 1) * w, d, width=w - 0.03, color=col, label=f"Input {lv} dBFS", zorder=3)
ax1.axhline(0, color=MUTED, linewidth=1, zorder=4)
ax1.set_xticks(x, [names[f] for f in freqs]); ax1.grid(axis="x", visible=False)
ax1.set_ylabel("Open − official, fundamental (dB)")
ax1.set_title("Low notes come out quieter")
ax1.legend(loc="lower right", fontsize=9.5)

lv = -12
offs = [-0.12, 0.12]
for (run, col, lab), o in zip((("A1", OFFICIAL, "Official"), ("B1", OURS, "Open")), offs):
    h3 = [res[(f, lv)][run][1] - res[(f, lv)][run][0] for f in freqs]
    ax2.scatter(x + o, h3, s=70, color=col, edgecolor=SURFACE, linewidth=2, zorder=3, label=lab)
for i, f in enumerate(freqs):
    a = res[(f, lv)]["A1"]; b = res[(f, lv)]["B1"]
    ax2.plot([i + offs[0], i + offs[1]], [a[1] - a[0], b[1] - b[0]], color=GRID, linewidth=2, zorder=2)
ax2.set_xticks(x, [names[f] for f in freqs]); ax2.grid(axis="x", visible=False)
ax2.set_ylabel("3rd harmonic re fundamental (dB)")
ax2.set_title("…with more distortion (input −12 dBFS)")
ax2.legend(loc="lower left", fontsize=9.5)
caption(fig, "1 s held sine tones on the open strings. Higher on the right chart = more 3rd-harmonic distortion.")
fig.tight_layout(rect=(0, 0.04, 1, 1)); fig.savefig(OUT / "3_low_notes.png", dpi=150); plt.close(fig)

# ---- 4. null tests ----
rows = []
for s in segs:
    a = seg("A1", s); core = slice(PAD + 2400, PAD + s["len"] - 2400)
    A = a[core]
    vals = []
    for n in ("A2", "B1"):
        B = aligned(n, s)[core]
        vals.append(10*np.log10(np.sum((B - A)**2) / np.sum(A**2)))
    rows.append((s["name"], *vals))
label = lambda n: (n.replace("sweep", "Sweep").replace("pink", "Pink noise") if not n.startswith("tone")
                   else f"{names[float(n.split()[1])]}, {n.split()[2]} dBFS")
fig, ax = plt.subplots(figsize=(10, 7.5))
y = np.arange(len(rows))[::-1]
for (n, rep, ab), yy in zip(rows, y):
    ax.plot([rep, ab], [yy, yy], color=GRID, linewidth=2, zorder=2)
ax.scatter([r[1] for r in rows], y, s=60, color=REPEAT, edgecolor=SURFACE, linewidth=2, zorder=3, label="Official vs official (repeatability)")
ax.scatter([r[2] for r in rows], y, s=60, color=OURS, edgecolor=SURFACE, linewidth=2, zorder=3, label="Open vs official")
ax.set_yticks(y, [label(r[0]) for r in rows]); ax.grid(axis="y", visible=False)
ax.set_xlabel("Null residual re signal (dB): lower = more alike")
ax.set_title("Null test: rig repeatability vs the difference between conversions")
ax.legend(loc="upper left", fontsize=9.5)
caption(fig, "Each recording aligned to official run 1 (sub-sample), subtracted, residual energy relative to the signal.")
fig.tight_layout(rect=(0, 0.03, 1, 1)); fig.savefig(OUT / "4_null_test.png", dpi=150); plt.close(fig)

# ---- 5. waveform close-up, E1 at -12 dBFS ----
s = next(s for s in tones if s["name"] == "tone 41.2 -12")
a = seg("A1", s); b = aligned("B1", s)
start = PAD + 24000; n = int(2.5 * SR / 41.2)
tt = np.arange(n) / SR * 1000
fig, ax = plt.subplots(figsize=(11, 4.9))
ax.plot(tt, a[start:start+n] * 1000, color=OFFICIAL, label="Official")
ax.plot(tt, b[start:start+n] * 1000, color=OURS, label="Open")
ax.set_xlabel("Time (ms)"); ax.set_ylabel("Output (×10⁻³ full scale)")
ax.set_title("E1 (41 Hz) at −12 dBFS input: the open conversion's peaks are lower")
ax.legend(loc="lower right", bbox_to_anchor=(1, 1.0), ncol=2, fontsize=9.5)
caption(fig, "2.5 cycles of the held note, both recordings time-aligned.")
fig.tight_layout(rect=(0, 0.04, 1, 1)); fig.savefig(OUT / "5_waveform_E1.png", dpi=150); plt.close(fig)
print("wrote", sorted(p.name for p in OUT.glob("*.png")))
