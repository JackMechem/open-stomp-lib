"""Re-amp A/B: play a fixed test signal into the pedal for several AMP slots, record over USB."""
import json, sys, time
from pathlib import Path
import numpy as np
sys.path.insert(0, sys.argv[1])
from audio_io import SR, find, playrec
from open_stomp import Device, DeviceState, Module

OUT = Path(sys.argv[2]); OUT.mkdir(parents=True, exist_ok=True)
RUNS = json.loads(sys.argv[3]) if len(sys.argv) > 3 else [["A1", 35], ["B1", 36], ["A2", 35], ["B2", 36]]   # [name, amp model or null = all blocks off]

def fade(x, n=480):
    x[:n] *= np.linspace(0, 1, n); x[-n:] *= np.linspace(1, 0, n); return x

def build():
    """Test signal + a segment map (name, start, length) on the playback timeline."""
    segs, parts, pos = [], [], 0
    def add(name, x, gap=0.6):
        nonlocal pos
        segs.append({"name": name, "start": pos, "len": len(x)})
        parts.append(x.astype(np.float32)); pos += len(x)
        g = np.zeros(int(gap * SR), np.float32); parts.append(g); pos += len(g)
    T, f1, f2 = 4.0, 20.0, 20000.0
    t = np.arange(int(T * SR)) / SR
    L = T / np.log(f2 / f1)
    sweep = np.sin(2 * np.pi * f1 * L * (np.exp(t / L) - 1))
    for db in (-36, -24, -12, -6):
        add(f"sweep {db}", fade(sweep * 10 ** (db / 20)))
    for f in (41.2, 55.0, 73.4, 98.0, 196.0):
        for db in (-24, -12, -6):
            tt = np.arange(int(1.0 * SR)) / SR
            add(f"tone {f} {db}", fade(np.sin(2 * np.pi * f * tt) * 10 ** (db / 20)), gap=0.3)
    rng = np.random.default_rng(1)
    w = rng.standard_normal(3 * SR); W = np.fft.rfft(w); W[1:] /= np.sqrt(np.arange(1, len(W))); p = np.fft.irfft(W, len(w))
    add("pink -18", fade(p / np.sqrt(np.mean(p ** 2)) * 10 ** (-18 / 20)))
    return np.concatenate(parts), segs, {"sweep_T": T, "f1": f1, "f2": f2}

sig, segs, meta = build()
if not (OUT / "map.json").exists():
    np.save(OUT / "played.npy", sig)
    json.dump({"segments": segs, "sr": SR, **meta, "runs": RUNS}, open(OUT / "map.json", "w"), indent=1)
assert np.array_equal(np.load(OUT / "played.npy"), sig), "test signal changed"
print(f"test signal {len(sig)/SR:.1f} s, {len(segs)} segments")

mtrack, pedal = find("M-Track"), find("STOMPBOX")
st = DeviceState()
with Device() as dev:
    tt = time.time()
    while time.time() - tt < 20:
        u = dev.recv(timeout=0.3)
        if u: st.feed(u)
        if st.live_known and all(p is not None for p in st.presets): break
    saved = {m: st.live.module(m) for m in Module.EFFECTS}
    try:
        for m, s in saved.items():
            dev.set_head(m, s.model, False)
        for name, model in RUNS:
            if model is not None:
                dev.set_head(Module.AMP, model, True)
            time.sleep(1.0)
            dev.drain()
            print(f"run {name}: " + (f"AMP model {model} ({st.model_name(Module.AMP, model)})" if model is not None else "all blocks off (dry)") + " …", flush=True)
            rec = playrec(sig, mtrack, pedal, pre=0.5, post=1.0)
            np.save(OUT / f"{name}.npy", rec[:, 0])
            print(f"   recorded {len(rec)/SR:.1f} s, peak {20*np.log10(np.abs(rec[:,0]).max()+1e-12):.1f} dBFS", flush=True)
    finally:
        for m, s in saved.items():
            dev.set_head(m, s.model, s.on)
        time.sleep(0.3)
        print("blocks restored")
