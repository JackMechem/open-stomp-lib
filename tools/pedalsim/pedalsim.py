"""Pedal simulator: y = DCblock( G2 * f( G1 * x ) ), f piecewise-linear, fitted from re-amp recordings."""
import json, sys
import numpy as np
from open_stomp import nam

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from paths import D, R, SVT
m = json.load(open(R + "map.json")); SR = m["sr"]; segs = m["segments"]
played = np.load(R + "played.npy").astype(np.float64)

PEDAL_TAPS = 512   # the pedal runs only the first 512 impulse-response taps of each filter
def taps(data):
    """Uploaded layout (reversed IR) -> the taps the pedal actually runs, still in uploaded layout."""
    d = np.asarray(data, dtype=np.float64).copy()
    d[:1024 - PEDAL_TAPS] = 0; d[1024:2048 - PEDAL_TAPS] = 0
    return d[:1024], d[1024:]
OFF = np.concatenate([np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g1.bin", "<f4"),
                      np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g2.bin", "<f4")])
DATA = {"A": OFF, "B": np.array(nam.convert(SVT).data), "C": np.load(R + "clean_data.npy")}
REC = {k: np.load(R + f"{k}1.npy").astype(np.float64) for k in DATA} | {"A2": np.load(R + "A2.npy").astype(np.float64)}

def fftconv(x, h):
    n = 1 << int(np.ceil(np.log2(len(x) + len(h))))
    return np.fft.irfft(np.fft.rfft(x, n) * np.fft.rfft(h, n), n)[:len(x)]

def dc_block(x):
    # same biquad as the app's DC blocker, vectorised via lfilter-free recursion in blocks
    b0, b1, b2, a1, a2 = 0.9953827, -1.990765, 0.9953827, -1.990744, 0.9907867
    y = np.empty_like(x); x1 = x2 = y1 = y2 = 0.0
    for i in range(len(x)):
        v = x[i]; y0 = b0*v + b1*x1 + b2*x2 - a1*y1 - a2*y2; y[i] = y0; x2, x1, y2, y1 = x1, v, y1, y0
    return y

KNOTS = np.concatenate([-2.0**np.arange(8, -3, -1), [0.0], 2.0**np.arange(-2, 9)])   # ±0.25 … ±256
def hats(u):
    """Piecewise-linear basis: column j is the hat around KNOTS[j] (linear extrapolation at the ends)."""
    k = KNOTS; J = len(k); out = np.zeros((len(u), J))
    idx = np.clip(np.searchsorted(k, u) - 1, 0, J - 2)
    t = (u - k[idx]) / (k[idx + 1] - k[idx])
    rows = np.arange(len(u))
    out[rows, idx] = 1 - t; out[rows, idx + 1] = t
    return out

# segment mask on the played timeline, with per-segment weights
mask = np.zeros(len(played), bool); weight = np.zeros(len(played))
for s in segs:
    a, b = s["start"] + 2400, s["start"] + s["len"] - 2400
    mask[a:b] = True
SEL = np.flatnonzero(mask)[::3]      # decimate rows for the fit

def columns(data, flip):
    g1, g2 = taps(data)
    h1, h2 = (g1[::-1], g2[::-1]) if flip else (g1, g2)
    u = fftconv(played, h1)
    H = hats(u)
    return np.stack([fftconv(H[:, j], h2) for j in range(H.shape[1])], 1), u

def coarse_offset(rec):
    """Where the played timeline starts in a recording (cross-correlation with the first sweep)."""
    s = segs[0]; a = played[s["start"]:s["start"] + SR]; b = rec[:s["start"] + 3 * SR]
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    return int(np.argmax(c[: 3 * SR])) - s["start"]

def align(pred, rec):
    """Integer delay of rec relative to pred (rec[n + d] ≈ pred[n]), refined around the coarse offset."""
    d0 = coarse_offset(rec)
    best, bd = -np.inf, d0
    a = pred[mask]
    for d in range(d0 - 600, d0 + 601):
        v = np.dot(a, rec[np.flatnonzero(mask) + d])
        if abs(v) > best: best, bd = abs(v), d
    return bd

def shift(x, d):
    X = np.fft.rfft(x); f = np.fft.rfftfreq(len(x))
    return np.fft.irfft(X * np.exp(-2j*np.pi*f*d), len(x))

def run_offset(pred, rec):
    """Where the played timeline starts in a recording, found with the predicted output over the
    whole signal (robust when the output doesn't resemble the input)."""
    a = pred.copy(); a[~mask] = 0
    b = rec[: len(a) + 3 * SR]
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)[: 3 * SR]
    return int(np.argmax(np.abs(c)))

def aligned_target(pred, rec):
    """The recording mapped onto the played timeline segment by segment (sub-sample), to undo clock
    drift between the playback and recording interfaces. Returns (target, per-segment delays)."""
    d0 = run_offset(pred, rec)
    tgt = np.zeros(len(played)); delays = []
    for s in segs:
        a, b = s["start"], s["start"] + s["len"]
        p = pred[a:b]; best, bd = -np.inf, d0
        for d in range(d0 - 300, d0 + 301):
            v = np.dot(p, rec[a + d:b + d])
            if v > best: best, bd = v, d
        c = [np.dot(p, rec[a + bd + k:b + bd + k]) for k in (-1, 0, 1)]
        den = c[0] - 2 * c[1] + c[2]; frac = 0.5 * (c[0] - c[2]) / den if den else 0.0
        seg = rec[a + bd - 64:b + bd + 64]
        tgt[a:b] = shift(seg, -frac)[64:64 + (b - a)]
        delays.append(bd + frac)
    return tgt, delays

def fit(key, flip):
    Z, u = columns(DATA[key], flip)
    rec = REC[key]
    g1, g2 = taps(DATA[key]); h2 = g2[::-1] if flip else g2
    tgt, delays = aligned_target(fftconv(u, h2), rec)
    d = delays
    y = tgt[SEL]
    # equal weight per segment: scale rows by 1/rms(recording segment)
    w = np.ones(len(SEL))
    for s in segs:
        a, b = s["start"] + 2400, s["start"] + s["len"] - 2400
        sel = (SEL >= a) & (SEL < b)
        w[sel] = 1 / (np.sqrt(np.mean(tgt[a:b] ** 2)) + 1e-9)
    A = Z[SEL] * w[:, None]
    c, *_ = np.linalg.lstsq(A, y * w, rcond=None)
    return c, d, u

def predict(key, c, flip):
    Z, _ = columns(DATA[key], flip)
    return Z @ c

def nulls(pred, rec, d=None):
    tgt, _ = aligned_target(pred, rec)
    out = {}
    for s in segs:
        a, b = s["start"] + 2400, s["start"] + s["len"] - 2400
        p, r = pred[a:b], tgt[a:b]
        out[s["name"]] = 10 * np.log10(np.sum((p - r) ** 2) / np.sum(r ** 2))
    return out

if __name__ == "__main__":
    for flip in (True, False):
        c, d, u = fit("A", flip)
        res = nulls(predict("A", c, flip), REC["A"], d)
        print(f"taps {'reversed (G1IR order)' if flip else 'as uploaded'}: delays {d[0]:.1f} … {d[-1]:.1f} samples, fit on official: mean null {np.mean(list(res.values())):6.1f} dB")
    flip = True
    c, d, u = fit("A", flip)
    np.save(R + "pedalsim_f.npy", np.stack([KNOTS, c]))
    print("\nf(u) at knots:", " ".join(f"{k:g}:{v:+.3g}" for k, v in zip(KNOTS, c)))
    print("\nper-segment null, simulator vs recording (fit on official only; lower = better):")
    rep = nulls(REC["A2"][0:0] if False else REC["A"], REC["A2"], 0)
    print(f"{'segment':16} {'official(fit)':>13} {'open app':>9} {'clean':>7} | {'rig repeat':>10}")
    pr = {k: nulls(predict(k, c, flip), REC[k], d) for k in "ABC"}
    for s in segs:
        n = s["name"]; print(f"{n:16} {pr['A'][n]:13.1f} {pr['B'][n]:9.1f} {pr['C'][n]:7.1f} |")
    for k in "ABC":
        print(f"mean {k}: {np.mean(list(pr[k].values())):.1f} dB")
