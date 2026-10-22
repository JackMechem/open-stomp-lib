"""Feature-metric error vs the real NAM for simulated outputs (same features as nam_features.py)."""
import json, sys
import numpy as np, torch
sys.path.insert(0, __file__.rsplit("/", 1)[0])
import optimize as O
from optimize import SR, segs, R, D, nam, SVT
m = json.load(open(R + "map.json")); F = json.load(open(R + "nam_features.json"))
BANDS = F["bands"]; PAD = 2000
T, f1, f2 = m["sweep_T"], m["f1"], m["f2"]
t = np.arange(int(T*SR))/SR; Lw = T/np.log(f2/f1)
sw = np.sin(2*np.pi*f1*Lw*(np.exp(t/Lw)-1)); INV = np.fft.rfft(sw[::-1]*np.exp(-t/Lw), 1 << 20)
def sweep_response(x, level_db):
    h = np.fft.irfft(np.fft.rfft(x, 1 << 20) * INV, 1 << 20)
    k = int(np.argmax(np.abs(h))); lin = h[k-2000:k+16000] * np.hanning(18000)
    H = np.abs(np.fft.rfft(lin, 1 << 16))**2; fr = np.fft.rfftfreq(1 << 16, 1/SR)
    return np.array([10*np.log10(np.mean(H[(fr >= f/2**(1/6)) & (fr < f*2**(1/6))])) for f in BANDS]) - level_db
def tone_feats(x, f):
    X = np.abs(np.fft.rfft(x*np.hanning(len(x)))); fr = np.fft.rfftfreq(len(x), 1/SR)
    h = [20*np.log10(X[np.argmin(abs(fr - f*k))] + 1e-12) for k in (1, 3, 5)]
    return np.array([h[0], h[1] - h[0], h[2] - h[0]])
def features(y):
    yp = np.concatenate([np.zeros(PAD), y, np.zeros(2*PAD)]); out = {}
    for s in segs:
        x = yp[s["start"]: s["start"] + s["len"] + 2*PAD]
        if s["name"].startswith("sweep"): out[s["name"]] = sweep_response(x[PAD-200:PAD+s["len"]+400], int(s["name"].split()[1]))
        elif s["name"].startswith("tone"): out[s["name"]] = tone_feats(x[PAD+9600:PAD+s["len"]-4800], float(s["name"].split()[1]))
    return out
nf = {k: np.array(v) for k, v in F["nam"].items()}; off = F["offset_db"]
def err(fe):
    sweep = np.concatenate([fe[k] - (nf[k] + off) for k in nf if k.startswith("sweep")])
    fund = np.array([fe[k][0] - (nf[k][0] + off) for k in nf if k.startswith("tone")])
    harm = np.concatenate([fe[k][1:] - nf[k][1:] for k in nf if k.startswith("tone")])
    r = lambda v: np.sqrt(np.mean(v**2))
    return r(sweep), r(fund), r(harm), r(np.concatenate([sweep, fund, harm]))
def sim(data):
    with torch.no_grad(): y, _ = O.simulate(*map(torch.tensor, O.taps_ir(data)))
    return y.numpy()[:len(O.played)]
if __name__ == "__main__":
    DATA = {"official": np.concatenate([np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g1.bin", "<f4"), np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g2.bin", "<f4")]),
            "ours (app)": np.array(nam.convert(SVT).data), "ours (clean)": np.load(R + "clean_data.npy")}
    for p in sys.argv[1:]: DATA[p.rsplit("/", 1)[-1]] = np.load(p)
    rec = {"official": "A1", "ours (app)": "B1", "ours (clean)": "C1"}
    print(f"{'':28} {'freq resp':>10} {'note level':>11} {'harmonics':>10} {'overall':>8}")
    for k, d in DATA.items():
        e = err(features(sim(d))); print(f"{k + ' (sim)':28} " + " ".join(f"{v:{w}.2f}" for v, w in zip(e, (10, 11, 10, 8))))
        if k in rec:
            e = err({n: np.array(v) for n, v in F["rec"][rec[k]].items()}); print(f"{k + ' (recorded)':28} " + " ".join(f"{v:{w}.2f}" for v, w in zip(e, (10, 11, 10, 8))))
