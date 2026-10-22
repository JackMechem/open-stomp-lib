"""Score each pedal conversion against the real NAM model on perceptual (magnitude) features."""
import json, sys, time
from pathlib import Path
import numpy as np
from open_stomp import nam

D = Path(sys.argv[1]); MODEL = sys.argv[2]
m = json.load(open(D / "map.json")); SR = m["sr"]
played = np.load(D / "played.npy").astype(np.float64)
RUNS = ("A1", "B1", "A2", "B2", "C1", "C2", "A3", "DRY")
runs = {n: np.load(D / f"{n}.npy").astype(np.float64) for n in RUNS}
segs = m["segments"]

def xcorr_lag(a, b, maxlag):
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    c = np.concatenate([c[-maxlag:], c[:maxlag + 1]]); return int(np.argmax(c)) - maxlag

first = segs[0]
offsets = {n: first["start"] + xcorr_lag(played[first["start"]:first["start"] + SR], r[:first["start"] + 3*SR], 3*SR - 1) - first["start"] for n, r in runs.items()}
PAD = 2000
def rec_seg(sig, off, s):
    o = int(round(off)) + s["start"] - PAD
    return sig[o:o + s["len"] + 2*PAD]

# ---- feature extraction ----
T, f1, f2 = m["sweep_T"], m["f1"], m["f2"]
t = np.arange(int(T*SR))/SR; L = T/np.log(f2/f1)
sw = np.sin(2*np.pi*f1*L*(np.exp(t/L)-1)); inv = sw[::-1]*np.exp(-t/L)
INV = np.fft.rfft(inv, 1 << 20)
norm_lv = None
BANDS = [31.5*2**(i/3) for i in range(28)]          # 31.5 Hz .. ~16 kHz
def sweep_response(x, level_db):
    h = np.fft.irfft(np.fft.rfft(x, 1 << 20) * INV, 1 << 20)
    k = int(np.argmax(np.abs(h))); lin = h[k-2000:k+16000] * np.hanning(18000)
    H = np.abs(np.fft.rfft(lin, 1 << 16))**2; fr = np.fft.rfftfreq(1 << 16, 1/SR)
    return np.array([10*np.log10(np.mean(H[(fr >= f/2**(1/6)) & (fr < f*2**(1/6))])) for f in BANDS]) - level_db

def tone_feats(x, f):
    X = np.abs(np.fft.rfft(x*np.hanning(len(x)))); fr = np.fft.rfftfreq(len(x), 1/SR)
    h = [20*np.log10(X[np.argmin(abs(fr - f*k))] + 1e-12) for k in (1, 3, 5)]
    return np.array([h[0], h[1] - h[0], h[2] - h[0]])

def features(get):
    """get(s) -> the segment (with PAD) for one run. Returns dict name -> feature vector."""
    out = {}
    for s in segs:
        x = get(s)
        if s["name"].startswith("sweep"):
            out[s["name"]] = sweep_response(x[PAD-200:PAD+s["len"]+400], int(s["name"].split()[1]))
        elif s["name"].startswith("tone"):
            out[s["name"]] = tone_feats(x[PAD+9600:PAD+s["len"]-4800], float(s["name"].split()[1]))
    return out

rec = {n: features(lambda s, n=n: rec_seg(runs[n], offsets[n], s)) for n in RUNS}

# dry path shape (linear part at -24 dBFS sweep, normalised at 1 kHz) -> applied to the NAM render
dry = rec["DRY"]["sweep -24"]; dry_shape = dry - dry[np.argmin(np.abs(np.array(BANDS) - 1000))]
print("dry path (rig) response re 1 kHz, dB:", " ".join(f"{f:.0f}:{v:+.1f}" for f, v in zip(BANDS, dry_shape) if f < 200 or f > 8000))
# turn the band shape into a smooth linear-phase EQ applied to the render
def apply_dry(y):
    n = 1 << int(np.ceil(np.log2(len(y)))); fr = np.fft.rfftfreq(n, 1/SR)
    g = np.interp(np.log10(np.maximum(fr, 1)), np.log10(BANDS), dry_shape, left=dry_shape[0], right=dry_shape[-1])
    return np.fft.irfft(np.fft.rfft(y, n) * 10**(g/20), n)[:len(y)]

model = nam.Model.load(MODEL)
def dc_block(x):
    b0, b1, b2, a1, a2 = 0.9953827, -1.990765, 0.9953827, -1.990744, 0.9907867
    out = [0.0]*len(x); x1 = x2 = y1 = y2 = 0.0
    for i, v in enumerate(x.tolist()):
        y0 = b0*v + b1*x1 + b2*x2 - a1*y1 - a2*y2; out[i] = y0; x2, x1, y2, y1 = x1, v, y1, y0
    return np.array(out)

def nam_features(gin_db):
    model.reset()
    y = np.array(model.process((played * 10**(gin_db/20)).astype(np.float32).tolist()), dtype=np.float64)
    y = apply_dry(dc_block(y))
    yp = np.concatenate([np.zeros(PAD), y, np.zeros(2*PAD)])
    return features(lambda s: yp[s["start"]: s["start"] + s["len"] + 2*PAD])

ABS = lambda name: True   # all sweep bands and tone fundamentals are absolute levels; harmonic ratios are relative
def compare(nf, which):
    """Fit one dB offset (output gain) shared by `which`, return (offset, per-run errors)."""
    diffs = []
    for n in which:
        for k, v in nf.items():
            if k.startswith("sweep"): diffs.append(rec[n][k] - v)
            else: diffs.append(np.array([rec[n][k][0] - v[0]]))
    off = float(np.mean(np.concatenate(diffs)))
    err = {}
    for n in [r for r in RUNS if r != "DRY"]:
        sweep = np.concatenate([rec[n][k] - (nf[k] + off) for k in nf if k.startswith("sweep")])
        fund = np.array([rec[n][k][0] - (nf[k][0] + off) for k in nf if k.startswith("tone")])
        harm = np.concatenate([rec[n][k][1:] - nf[k][1:] for k in nf if k.startswith("tone")])
        err[n] = {"sweep": np.sqrt(np.mean(sweep**2)), "fund": np.sqrt(np.mean(fund**2)), "harm": np.sqrt(np.mean(harm**2))}
        err[n]["all"] = np.sqrt(np.mean(np.concatenate([sweep, fund, harm])**2))
    return off, err

t0 = time.time(); grid = {}; cache = {}
def evaluate(g):
    nf = nam_features(g); cache[g] = nf
    off, err = compare(nf, ("A1", "B1", "C1"))
    grid[g] = (err["A1"]["all"]**2 + err["B1"]["all"]**2 + err["C1"]["all"]**2) / 3
    return grid[g]
for g in range(-24, 10, 3):
    print(f"  input gain {g:+4d} dB -> joint rms error {np.sqrt(evaluate(g)):.2f} dB", flush=True)
b = min(grid, key=grid.get)
for g in np.arange(b - 2.5, b + 2.51, 0.5):
    if float(g) not in grid: evaluate(float(g))
best = min(grid, key=grid.get)
off, err = compare(cache[best], ("A1", "B1", "C1"))
print(f"\nbest input gain {best:+.1f} dB, output offset {off:+.1f} dB ({time.time()-t0:.0f}s)")
print(f"\nrms error vs the real NAM (dB)   {'freq resp':>10} {'note level':>11} {'harmonics':>10} {'overall':>8}")
for n, lab in (("A1", "official run 1"), ("A2", "official run 2"), ("A3", "official run 3"), ("B1", "ours (app) run 1"), ("B2", "ours (app) run 2"), ("C1", "ours (clean) run 1"), ("C2", "ours (clean) run 2")):
    e = err[n]; print(f"  {lab:30} {e['sweep']:10.2f} {e['fund']:11.2f} {e['harm']:10.2f} {e['all']:8.2f}")
json.dump({"gin_db": best, "offset_db": off, "bands": BANDS,
           "nam": {k: v.tolist() for k, v in cache[best].items()},
           "rec": {n: {k: v.tolist() for k, v in rec[n].items()} for n in rec}}, open(D / "nam_features.json", "w"))
