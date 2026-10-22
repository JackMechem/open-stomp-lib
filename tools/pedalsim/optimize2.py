"""Optimise G1/G2 as smooth minimum-phase curves so the simulated pedal sounds like the real NAM.

G1, G2: log-magnitude at 41 log-spaced control points (20 Hz … 20 kHz, ¼ octave), linearly
interpolated in log-frequency, made minimum-phase (cepstrum, N = 8192) and truncated to 1024 taps —
the same structure the converter uploads.
Training: the re-amp sweeps and tones plus synthetic pink noise at three levels (own seeds).
Hold-out: the recorded pink-noise segment.
usage: optimize2.py <init: A|B|C> <steps> <out.npy>
"""
import sys, time
import numpy as np, torch
sys.path.insert(0, __file__.rsplit("/", 1)[0])
import optimize as O
from optimize import SR, segs, played, nam, SVT, R, D

N = 8192; FR = np.fft.rfftfreq(N, 1 / SR)
CP = np.log10(20) + np.arange(41) * np.log10(2) / 4                     # control points, log10 Hz
lf = np.log10(np.maximum(FR, 1e-3))
# interpolation matrix: bins x control points (linear in log-f, constant outside)
W = np.zeros((len(FR), len(CP)))
idx = np.clip(np.searchsorted(CP, lf) - 1, 0, len(CP) - 2)
t = np.clip((lf - CP[idx]) / (CP[idx + 1] - CP[idx]), 0, 1)
W[np.arange(len(FR)), idx] = 1 - t; W[np.arange(len(FR)), idx + 1] = t
W = torch.tensor(W)
FOLD = torch.zeros(N); FOLD[0] = 1; FOLD[1:N // 2] = 2; FOLD[N // 2] = 1

def minphase(logmag):                   # natural-log magnitude on the rfft grid -> 1024 taps
    c = torch.fft.irfft(logmag.to(torch.complex128), N) * FOLD
    return torch.fft.irfft(torch.exp(torch.fft.rfft(c)), N)[:O.PEDAL_TAPS]
def curve_of(h):                        # initial control-point values from existing taps
    H = np.log(np.abs(np.fft.rfft(h, N)) + 1e-9)
    return np.array([np.mean(H[(lf >= c - 0.0625) & (lf < c + 0.0625)]) for c in CP])

# ---------- training material: played + synthetic pink ----------
def pink(n, seed):
    r = np.random.default_rng(seed); X = np.fft.rfft(r.standard_normal(n)); f = np.fft.rfftfreq(n, 1 / SR)
    X[1:] /= np.sqrt(f[1:]); X[0] = 0; X[f < 20] = 0; x = np.fft.irfft(X, n); return x / np.sqrt(np.mean(x ** 2))
extra, xsegs, pos = [], [], len(played)
for i, lvl in enumerate((-30, -18, -6)):
    x = np.concatenate([np.zeros(SR // 2), pink(3 * SR, 1000 + i) * 10 ** (lvl / 20) * 0.5])
    extra.append(x); xsegs.append({"name": f"synth pink {lvl}", "start": pos + SR // 2, "len": 3 * SR}); pos += len(x)
O.played = np.concatenate([played] + extra + [np.zeros(SR // 2)])
O.X = torch.tensor(O.played); O.NF = 1 << int(np.ceil(np.log2(len(O.played) + 2048))); O.XF = torch.fft.rfft(O.X, O.NF)

if __name__ == "__main__":
    init, steps, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    DATA = {"A": np.concatenate([np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g1.bin", "<f4"),
                                 np.fromfile(D + "namconv/SVT_Gain_10_Ultra_Lo_and_Hi_MD_421/cb_g2.bin", "<f4")]),
            "B": np.array(nam.convert(SVT).data), "C": np.load(R + "clean_data.npy")}
    tgt = O.target(); np.save(R + "opt2_target.npy", tgt.astype(np.float32))
    TRAIN = [s for s in segs if not s["name"].startswith("pink")] + xsegs
    L, LH = O.make_loss(tgt, TRAIN), O.make_loss(tgt, O.HOLD)
    with torch.no_grad():
        for k, d in DATA.items():
            y, u = O.simulate(*map(torch.tensor, O.taps_ir(d)))
            print(f"start {k} (raw taps): train {L(y).item():.4f}  hold-out {LH(y).item():.4f}")
    g1, g2 = O.taps_ir(DATA[init])
    c1, c2 = (torch.tensor(curve_of(g), requires_grad=True) for g in (g1, g2))
    opt = torch.optim.Adam([c1, c2], lr=0)
    t0 = time.time(); best = (np.inf, None)
    for it in range(steps):
        opt.param_groups[0]["lr"] = 0.05 * min(1, (it + 1) / 10) * (0.5 * (1 + np.cos(np.pi * it / steps)))
        opt.zero_grad()
        h1, h2 = minphase(W @ c1), minphase(W @ c2)
        y, u = O.simulate(h1, h2)
        smooth = (torch.diff(c1, 2).pow(2).mean() + torch.diff(c2, 2).pow(2).mean())
        loss = L(y) + 1e-3 * torch.relu(u.abs() - 100).pow(2).mean() + 1e-3 * smooth
        loss.backward(); opt.step()
        if it % 10 == 0 or it == steps - 1:
            with torch.no_grad(): lh = LH(y).item()
            print(f"step {it:4d}  train {loss.item():.4f}  hold-out {lh:.4f}  max|u| {u.abs().max():.0f}  ({time.time()-t0:.0f}s)", flush=True)
        if loss.item() < best[0]: best = (loss.item(), (h1.detach().numpy().copy(), h2.detach().numpy().copy(), c1.detach().numpy().copy(), c2.detach().numpy().copy()))
    np.save(out, O.to_upload(*best[1][:2])); np.save(out.replace(".npy", "_curves.npy"), np.stack(best[1][2:]))
    print("saved", out)
