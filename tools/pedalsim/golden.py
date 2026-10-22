"""Golden reference data for the Rust port of the optimiser (open-stomp-lib `nam::optimise`).

Runs the exact torch code that produced the hardware-validated upload (optimize.py / optimize2.py) on
small inputs and writes the results as little-endian f64 files plus a JSON manifest.
usage: golden.py <out dir>   (normally open-stomp-lib/tests/data/optim)
"""
import json, sys
from pathlib import Path
import numpy as np, torch
from scipy.signal import lfilter
sys.path.insert(0, __file__.rsplit("/", 1)[0])
import optimize as O
import optimize2 as O2
from open_stomp import nam

OUT = Path(sys.argv[1]); OUT.mkdir(parents=True, exist_ok=True)
LIB = OUT.parents[2]
TEST_NAM = str(LIB / "tests/data/nam/wavenet.nam")   # the repo's own (MIT) test model, never third-party data
man = {}
def save(name, a):
    a = np.ascontiguousarray(np.asarray(a, np.float64).ravel()); a.astype("<f8").tofile(OUT / f"{name}.bin"); man[name] = len(a)
rng = np.random.default_rng(7)

# ---------- f ----------
u = np.concatenate([O.KNOTS[np.abs(O.KNOTS) <= 64], np.linspace(-60, 60, 997), [-300.0, 300.0]])
ut = torch.tensor(u, requires_grad=True); v = O.f(ut); v.sum().backward()
save("f_u", u); save("f_v", v.detach()); save("f_dv", ut.grad)

# ---------- curves -> minimum phase ----------
C = np.array(nam.convert(TEST_NAM, measure="clean").data); g1, g2 = O.taps_ir(C)
c1, c2 = O2.curve_of(g1), O2.curve_of(g2)
save("co_h1", g1); save("co_c1", c1); save("co_h2", g2); save("co_c2", c2)
c = torch.tensor(c1 + 0.3 * rng.standard_normal(41), requires_grad=True)
h = O2.minphase(O2.W @ c); gh = rng.standard_normal(O.PEDAL_TAPS)
(h * torch.tensor(gh)).sum().backward()
save("mp_c", c.detach()); save("mp_h", h.detach()); save("mp_gh", gh); save("mp_gc", c.grad)

# ---------- simulator, penalty and loss on a short signal ----------
L = 20000
t = np.arange(L) / O.SR
x = 40 * np.sin(2 * np.pi * 70 * t) * np.linspace(0, 1, L) + 2 * rng.standard_normal(L)
O.played = x; O.X = torch.tensor(x); O.NF = 1 << int(np.ceil(np.log2(L + 2048))); O.XF = torch.fft.rfft(O.X, O.NF)
segs = [{"name": "a", "start": 0, "len": 9000}, {"name": "b", "start": 9000, "len": 11000}]
cA = c1 + 0.5 * rng.standard_normal(41); cB = c2 + 0.5 * rng.standard_normal(41)
with torch.no_grad():
    tgt = O.simulate(O2.minphase(O2.W @ torch.tensor(c1)), O2.minphase(O2.W @ torch.tensor(c2)))[0].numpy()
tgt = tgt + 1e-3 * rng.standard_normal(L)
loss = O.make_loss(tgt, segs)
save("sim_x", x); save("sim_tgt", tgt); man["sim_segs"] = [[s["start"], s["len"]] for s in segs]

h1 = torch.tensor(O2.minphase(O2.W @ torch.tensor(cA)).numpy(), requires_grad=True)
h2 = torch.tensor(O2.minphase(O2.W @ torch.tensor(cB)).numpy(), requires_grad=True)
y, uu = O.simulate(h1, h2)
gy = rng.standard_normal(L)
pen = 1e-3 * torch.relu(uu.abs() - 100).pow(2).mean()
spec = loss(y)
((y * torch.tensor(gy)).sum() + pen).backward()
save("sim_h1", h1.detach()); save("sim_h2", h2.detach()); save("sim_y", y.detach()); save("sim_u", uu.detach())
save("sim_gy", gy); save("sim_gh1", h1.grad); save("sim_gh2", h2.grad); man["sim_pen"] = pen.item()

yl = torch.tensor(y.detach().numpy(), requires_grad=True); sl = loss(yl); sl.backward()
man["loss"] = sl.item(); save("loss_gy", yl.grad)

# ---------- a few optimiser steps (the optimize2 loop) ----------
steps = 8
p1, p2 = torch.tensor(cA.copy(), requires_grad=True), torch.tensor(cB.copy(), requires_grad=True)
opt = torch.optim.Adam([p1, p2], lr=0); hist = []
for it in range(steps):
    opt.param_groups[0]["lr"] = 0.05 * min(1, (it + 1) / 10) * (0.5 * (1 + np.cos(np.pi * it / steps)))
    opt.zero_grad()
    y, uu = O.simulate(O2.minphase(O2.W @ p1), O2.minphase(O2.W @ p2))
    smooth = torch.diff(p1, 2).pow(2).mean() + torch.diff(p2, 2).pow(2).mean()
    tot = loss(y) + 1e-3 * torch.relu(uu.abs() - 100).pow(2).mean() + 1e-3 * smooth
    tot.backward(); opt.step(); hist.append(tot.item())
save("opt_c0", np.concatenate([cA, cB])); save("opt_loss", hist)
save("opt_c", np.concatenate([p1.detach().numpy(), p2.detach().numpy()]))

# ---------- target render (NAM -> DC blocker -> gain) on the test model ----------
m = nam.Model.load(TEST_NAM)
xr = 0.5 * np.sin(2 * np.pi * 110 * np.arange(4800) / O.SR)
yr = np.array(m.process((xr * 10 ** (-12 / 20)).astype(np.float32).tolist()), np.float64)
save("tgt_x", xr); save("tgt_y", lfilter(*O.DCB, yr) * O.cal["gout"])

# ---------- the re-amp test signal (deterministic parts) ----------
mp = json.load(open(O.R + "map.json")); played = np.load(O.R + "played.npy").astype(np.float64)
idx = np.sort(rng.choice(mp["segments"][-1]["start"], 4000, replace=False))
save("sig_idx", idx); save("sig_v", played[idx]); man["sig_len"] = len(played); man["sig_segs"] = mp["segments"]

json.dump(man, open(OUT / "golden.json", "w"), indent=1)
print("wrote", OUT, {k: v for k, v in man.items() if not isinstance(v, list)})
