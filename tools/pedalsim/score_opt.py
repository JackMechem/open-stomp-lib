"""Score re-amp runs in a directory against the real NAM on the feature metric."""
import json, sys
import numpy as np
sys.path.insert(0, __file__.rsplit("/", 1)[0])
import eval_feats as E
from pathlib import Path
DIR = Path(sys.argv[1]); RUNS = sys.argv[2].split(",")
played = np.load(DIR / "played.npy").astype(np.float64); segs = E.segs; SR = E.SR
def xcorr_lag(a, b, maxlag):
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    c = np.concatenate([c[-maxlag:], c[:maxlag + 1]]); return int(np.argmax(c)) - maxlag
def on_timeline(r):
    s = segs[0]; off = xcorr_lag(played[s["start"]:s["start"] + SR], r[:s["start"] + 3 * SR], 3 * SR - 1)
    return r[off:off + len(played)]
feats = {n: E.features(on_timeline(np.load(DIR / f"{n}.npy").astype(np.float64))) for n in RUNS}
def row(lab, fe):
    e = E.err(fe); print(f"{lab:34} " + " ".join(f"{v:{w}.2f}" for v, w in zip(e, (10, 11, 10, 8))))
print(f"{'error vs real NAM, dB rms':34} {'freq resp':>10} {'note level':>11} {'harmonics':>10} {'overall':>8}")
for n in RUNS: row(n + " (recorded now)", feats[n])
old = {n: {k: np.array(v) for k, v in E.F["rec"][n].items()} for n in ("A1", "C1")}
for n in old: row(n + " (recorded last session)", old[n])
d = np.concatenate([feats["A1"][k] - old["A1"][k] for k in feats["A1"] if k.startswith("sweep")])
print(f"\nrig drift: official now vs last session, sweep bands: mean {d.mean():+.2f} dB, rms {np.sqrt(np.mean(d**2)):.2f} dB")
json.dump({n: {k: v.tolist() for k, v in f.items()} for n, f in feats.items()}, open(DIR / "features.json", "w"))
