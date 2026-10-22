"""Optimise the uploaded G1/G2 taps so the simulated pedal sounds like the real NAM model.

Pedal simulator (fitted from re-amp recordings, see pedalsim.py):  y = G2 ⊛ f(G1 ⊛ x)
Target: NAM(x · g_in), DC-blocked, through the rig's dry-path response, times g_out (nam_calibration).
Loss: multi-resolution log-magnitude STFT distance on the sweeps and tones; the pink-noise segment
is held out for validation.
usage: optimize.py <init: A|B|C> <steps> <out.npy>
"""
import json, sys, time
import numpy as np, torch
from scipy.signal import lfilter
from open_stomp import nam
sys.path.insert(0, __file__.rsplit("/", 1)[0])

from paths import D, R, SVT
m = json.load(open(R + "map.json")); SR = m["sr"]; segs = m["segments"]
played = np.load(R + "played.npy").astype(np.float64)
cal = json.load(open(R + "nam_calibration.json")); feats = json.load(open(R + "nam_features.json"))
KNOTS, FC = np.load(R + "pedalsim_f.npy")
# above |u| = 32 the recordings barely sample f: hold it flat (saturated) there
FC = FC.copy(); FC[KNOTS > 32] = FC[KNOTS == 32][0]; FC[KNOTS < -32] = FC[KNOTS == -32][0]
torch.set_default_dtype(torch.float64)

# ---------- target ----------
DCB = ([0.9953827, -1.990765, 0.9953827], [1, -1.990744, 0.9907867])
def target():
    model = nam.Model.load(SVT)
    y = np.array(model.process((played * 10 ** (cal["gin_db"] / 20)).astype(np.float32).tolist()), np.float64)
    y = lfilter(*DCB, y)
    bands = np.array(feats["bands"]); dry = np.array(feats["rec"]["DRY"]["sweep -24"])
    shape = dry - dry[np.argmin(abs(bands - 1000))]
    n = 1 << int(np.ceil(np.log2(len(y)))); fr = np.fft.rfftfreq(n, 1 / SR)
    g = np.interp(np.log10(np.maximum(fr, 1)), np.log10(bands), shape)
    return np.fft.irfft(np.fft.rfft(y, n) * 10 ** (g / 20), n)[:len(y)] * cal["gout"]

# ---------- simulator ----------
X = torch.tensor(played); NF = 1 << int(np.ceil(np.log2(len(played) + 2048)))
XF = torch.fft.rfft(X, NF)
K = torch.tensor(KNOTS); C = torch.tensor(FC)
def f(u):
    i = torch.clamp(torch.searchsorted(K, u.detach()) - 1, 0, len(K) - 2)
    k0, k1 = K[i], K[i + 1]; t = (u - k0) / (k1 - k0)
    return C[i] * (1 - t) + C[i + 1] * t
def conv(xf, h): return torch.fft.irfft(xf * torch.fft.rfft(h, NF), NF)[:len(played)]
def simulate(h1, h2):
    u = conv(XF, h1)
    return conv(torch.fft.rfft(f(u), NF), h2), u
PEDAL_TAPS = 512      # the pedal runs only the first 512 impulse-response taps of each filter
def taps_ir(data):     # uploaded layout -> the impulse responses the pedal runs
    d = np.asarray(data, np.float64); return d[:1024][::-1][:PEDAL_TAPS].copy(), d[1024:][::-1][:PEDAL_TAPS].copy()
def to_upload(h1, h2):
    z = lambda h: np.concatenate([np.zeros(1024 - len(h)), np.asarray(h)[::-1]])
    return np.concatenate([z(h1), z(h2)]).astype(np.float32)

# ---------- loss ----------
TRAIN = [s for s in segs if not s["name"].startswith("pink")]
HOLD = [s for s in segs if s["name"].startswith("pink")]
def seg_slices(ss): return [(s["start"] + 2400, s["start"] + s["len"] - 2400) for s in ss]
RES = [(4096, 1024), (1024, 256), (256, 64)]
WIN = {n: torch.hann_window(n) for n, _ in RES}
def logspec(x, n, hop): return torch.stft(x, n, hop, window=WIN[n], return_complex=True).abs()
def make_loss(tgt, ss):
    parts = []
    for a, b in seg_slices(ss):
        t = torch.tensor(tgt[a:b]); eps = 1e-3 * t.pow(2).mean().sqrt()
        parts.append((a, b, eps, [torch.log(logspec(t, n, h) + eps * np.sqrt(n)) for n, h in RES]))
    def loss(y):
        tot = 0.0
        for a, b, eps, ts in parts:
            for (n, h), T in zip(RES, ts):
                tot = tot + (torch.log(logspec(y[a:b], n, h) + eps * np.sqrt(n)) - T).abs().mean()
        return tot / (len(parts) * len(RES))
    return loss

if __name__ == "__main__":
    init, steps, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    DATA = {"A": np.concatenate([np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g1.bin", "<f4"),
                                 np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g2.bin", "<f4")]),
            "B": np.array(nam.convert(SVT).data), "C": np.load(R + "clean_data.npy")}
    tgt = target(); np.save(R + "opt_target.npy", tgt.astype(np.float32))
    L, LH = make_loss(tgt, TRAIN), make_loss(tgt, HOLD)
    with torch.no_grad():
        for k, d in DATA.items():
            y, u = simulate(*map(torch.tensor, taps_ir(d)))
            print(f"start {k}: train loss {L(y).item():.4f}  hold-out {LH(y).item():.4f}  max|u| {u.abs().max():.0f}")
    g1, g2 = taps_ir(DATA[init]); s1, s2 = abs(g1).max(), abs(g2).max()
    p1, p2 = torch.tensor(g1 / s1, requires_grad=True), torch.tensor(g2 / s2, requires_grad=True)
    opt = torch.optim.Adam([p1, p2], lr=0)
    t0 = time.time(); best = (np.inf, None)
    for it in range(steps):
        opt.param_groups[0]["lr"] = 1e-2 * min(1, (it + 1) / 20) * (0.5 * (1 + np.cos(np.pi * it / steps)))
        opt.zero_grad()
        y, u = simulate(p1 * s1, p2 * s2)
        loss = L(y) + 1e-3 * torch.relu(u.abs() - 100).pow(2).mean()   # stay inside the fitted range of f
        loss.backward(); opt.step()
        if it % 10 == 0 or it == steps - 1:
            with torch.no_grad(): lh = LH(y).item()
            print(f"step {it:4d}  train {loss.item():.4f}  hold-out {lh:.4f}  max|u| {u.abs().max():.0f}  ({time.time()-t0:.0f}s)", flush=True)
        if loss.item() < best[0]: best = (loss.item(), ((p1 * s1).detach().numpy().copy(), (p2 * s2).detach().numpy().copy()))
    np.save(out, to_upload(*best[1]))
    print("saved", out)
