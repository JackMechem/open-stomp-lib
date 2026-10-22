"""What the pedal does with a user IR: the CAB block's measured impulse response (sweep deconvolution,
divided by the dry rig path) against what was uploaded.
usage: ir_reamp_analyze.py <dir from ir_reamp_prep + reamp_run> [DRY RUN ...]   (the first run is the dry reference)
"""
import json, sys
from pathlib import Path
import numpy as np

D = Path(sys.argv[1]); names = sys.argv[2:] or ["DRY", "PROBE", "VERB"]
m = json.load(open(D / "map.json")); SR = m["sr"]; segs = m["segments"]
played = np.load(D / "played.npy").astype(np.float64)
T, f1, f2 = m["sweep_T"], m["f1"], m["f2"]
t = np.arange(int(T * SR)) / SR; L = T / np.log(f2 / f1)
sweep = np.sin(2 * np.pi * f1 * L * (np.exp(t / L) - 1)); NF = 1 << 20
INV = np.fft.rfft(sweep[::-1] * np.exp(-t / L), NF)

def xcorr_lag(a, b, maxlag):
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    c = np.concatenate([c[-maxlag:], c[:maxlag + 1]]); return int(np.argmax(c)) - maxlag

def on_timeline(r):
    s = segs[0]; off = xcorr_lag(played[s["start"]:s["start"] + SR], r[:s["start"] + 3 * SR], 3 * SR - 1)
    return r[off:off + len(played)]

def response(rec, seg_name, n=1 << 15):
    """The linear impulse response of one sweep segment, starting 1000 samples before its peak."""
    s = next(s for s in segs if s["name"] == seg_name)
    x = rec[s["start"] - 200: s["start"] + s["len"] + 400]
    h = np.fft.irfft(np.fft.rfft(x, NF) * INV, NF)
    k = int(np.argmax(np.abs(h)))
    return h, k

runs = {n: on_timeline(np.load(D / f"{n}.npy").astype(np.float64)) for n in names}
prep = json.load(open(D / "ir_prep.json"))
N = 1 << 14
for seg in ("sweep -24", "sweep -12"):
    hd, kd = response(runs[names[0]], seg)   # the first run is the dry reference
    print(f"\n== {seg}: dry peak at {kd}")
    Hd = np.fft.rfft(hd[kd - 1000: kd - 1000 + N])
    for n in names[1:]:
        h, _ = response(runs[n], seg)
        # same time reference as the dry run: the CAB block's own delay and response stay in
        H = np.fft.rfft(h[kd - 1000: kd - 1000 + N])
        rel = np.fft.irfft(H / np.where(np.abs(Hd) > 1e-9 * np.abs(Hd).max(), Hd, 1), N)
        print(f"  {n}: relative response, strongest taps (index re dry, value):")
        top = np.argsort(-np.abs(rel))[:8]
        for i in sorted(top):
            print(f"     tap {i - 1000:6d}  {rel[i]:+.4f}")
        np.save(D / f"rel_{n}_{seg.split()[1]}.npy", rel)
    if "PROBE" in names:
        rel = np.load(D / f"rel_PROBE_{seg.split()[1]}.npy")
        k0 = int(np.argmax(np.abs(rel)))
        print(f"  probe: main impulse at {k0 - 1000} (pedal latency vs dry, samples); level {20*np.log10(abs(rel[k0])):+.2f} dB")
        for tap, v in prep["probe"].items():
            tap = int(tap)
            got = rel[k0 + tap - 3: k0 + tap + 4]
            j = int(np.argmax(np.abs(got)))
            print(f"     uploaded tap {tap:5d} = {v:+.3f}: measured {got[j] / rel[k0] * 1.0:+.4f} (×main), at offset {j - 3:+d}")
