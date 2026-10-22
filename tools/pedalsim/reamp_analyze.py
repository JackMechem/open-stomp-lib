"""Analyse re-amp recordings: per-segment alignment, level differences, null tests, sweep responses, harmonics."""
import json, sys
from pathlib import Path
import numpy as np

D = Path(sys.argv[1])
m = json.load(open(D / "map.json")); SR = m["sr"]
played = np.load(D / "played.npy").astype(np.float64)
runs = {name: np.load(D / f"{name}.npy").astype(np.float64) for name, _ in m["runs"]}
segs = m["segments"]

def xcorr_lag(a, b, maxlag):
    """Fractional lag of b relative to a (b[n + lag] ≈ a[n])."""
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    c = np.concatenate([c[-maxlag:], c[:maxlag + 1]])
    i = int(np.argmax(c)); y0, y1, y2 = c[max(i-1,0)], c[i], c[min(i+1,len(c)-1)]
    frac = 0.5 * (y0 - y2) / (y0 - 2*y1 + y2) if (y0 - 2*y1 + y2) != 0 else 0.0
    return i - maxlag + frac

def shift(x, d):
    """Delay x by d samples (fractional, via FFT)."""
    n = len(x); X = np.fft.rfft(x); f = np.fft.rfftfreq(n)
    return np.fft.irfft(X * np.exp(-2j*np.pi*f*d), n)

# global offset of each run: correlate the first second of the played sweep region with the recording
first = segs[0]
offsets = {}
for name, r in runs.items():
    probe = played[first["start"]:first["start"] + SR]
    lag = xcorr_lag(probe, r[:first["start"] + 3*SR], 3*SR - 1)
    offsets[name] = lag
print("start offsets (samples):", {k: round(v, 2) for k, v in offsets.items()})

ref = "A1"
pad = 2000
def seg(name, s, extra=0):
    o = int(round(offsets[name])) + s["start"] - pad
    return runs[name][o:o + s["len"] + 2*pad + extra]

rows = []
for s in segs:
    a = seg(ref, s)
    out = {"name": s["name"]}
    for name in runs:
        if name == ref: continue
        b = seg(name, s)
        lag = xcorr_lag(a, b, 400)
        b_al = shift(b, -lag)
        core = slice(pad + 2400, pad + s["len"] - 2400)     # skip edges/fades
        A, Bv = a[core], b_al[core]
        lvl = 10*np.log10(np.sum(Bv**2) / np.sum(A**2))
        null = 10*np.log10(np.sum((Bv - A)**2) / np.sum(A**2))
        g = np.dot(A, Bv) / np.dot(Bv, Bv)                    # gain-matched residual
        null_g = 10*np.log10(np.sum((g*Bv - A)**2) / np.sum(A**2))
        out[name] = (lvl, null, null_g, lag)
    rows.append(out)

print(f"\n{'segment':18} | {'A2 vs A1 (repeat)':>27} | {'B1 vs A1':>27} | {'B2 vs A1':>27}")
print(f"{'':18} | {'level  null  null(gain)':>27} | {'level  null  null(gain)':>27} | {'level  null  null(gain)':>27}")
for r in rows:
    cells = [f"{r[n][0]:+6.2f} {r[n][1]:6.1f} {r[n][2]:7.1f}" for n in ("A2", "B1", "B2")]
    print(f"{r['name']:18} | {cells[0]:>27} | {cells[1]:>27} | {cells[2]:>27}")

# sweep responses (Farina): deconvolve with the inverse filter, take the linear IR
T, f1, f2 = m["sweep_T"], m["f1"], m["f2"]
t = np.arange(int(T*SR))/SR; L = T/np.log(f2/f1)
sw = np.sin(2*np.pi*f1*L*(np.exp(t/L)-1)); inv = sw[::-1]*np.exp(-t/L)
def response(x):
    n = 1 << 20
    h = np.fft.irfft(np.fft.rfft(x, n) * np.fft.rfft(inv, n), n)
    k = int(np.argmax(np.abs(h))); lin = h[k-2000:k+16000] * np.hanning(18000)
    H = np.abs(np.fft.rfft(lin, 1 << 16)); fr = np.fft.rfftfreq(1 << 16, 1/SR)
    bands = []
    fc = 31.25
    while fc < 16000:
        sel = (fr >= fc/2**(1/6)) & (fr < fc*2**(1/6)); bands.append((fc, 10*np.log10(np.mean(H[sel]**2)))); fc *= 2**(1/3)
    return bands
print("\nsweep frequency response, B (ours) minus A (official), per 1/3 octave [dB] (A2-A1 in brackets = repeatability):")
for s in segs:
    if not s["name"].startswith("sweep"): continue
    resp = {n: response(seg(n, s)[pad - 200: pad + s["len"] + 400]) for n in runs}
    fcs = [fc for fc, _ in resp["A1"]]
    a = np.array([v for _, v in resp["A1"]]); a2 = np.array([v for _, v in resp["A2"]])
    b = (np.array([v for _, v in resp["B1"]]) + np.array([v for _, v in resp["B2"]])) / 2
    d = b - a; rep = a2 - a
    show = [i for i, fc in enumerate(fcs) if fc in (31.25,) or round(fc) in (40, 63, 100, 160, 250, 397, 630, 1000, 1587, 2520, 4000, 6350, 10079)]
    print(f"  {s['name']:10} rms {np.sqrt(np.mean(d**2)):.2f} dB (repeat {np.sqrt(np.mean(rep**2)):.2f}), max {np.max(np.abs(d)):.2f} dB at {fcs[int(np.argmax(np.abs(d)))]:.0f} Hz")
    print("     " + "  ".join(f"{fcs[i]:.0f}Hz:{d[i]:+.1f}[{rep[i]:+.1f}]" for i in show))

print("\ntone harmonics (dB re fundamental), A1 | B1:")
for s in segs:
    if not s["name"].startswith("tone"): continue
    f = float(s["name"].split()[1])
    out = []
    for n in ("A1", "B1"):
        x = seg(n, s)[pad + 9600: pad + s["len"] - 4800]
        X = np.abs(np.fft.rfft(x*np.hanning(len(x)))); fr = np.fft.rfftfreq(len(x), 1/SR)
        h = [X[np.argmin(abs(fr - f*k))] for k in range(1, 6)]
        out.append(f"H1 {20*np.log10(h[0]+1e-12):6.1f}  " + " ".join(f"{20*np.log10(v/h[0]+1e-12):6.1f}" for v in h[1:]))
    print(f"  {s['name']:16} {out[0]}  |  {out[1]}")
