"""NAM import level on the pedal: the slot-info `factor` field, and normalised imports against the
pedal's built-in bass amps. Uploads to USER AMP 09/10, re-amps, reports levels.
usage: nam_level_rig.py <dir with svtvr_Normal_norm.bin and svtvr_Lo_norm.bin>
"""
import json, subprocess, sys, time
from pathlib import Path
import numpy as np
from open_stomp import Device, DeviceState

D = Path(sys.argv[1]); HERE = Path(__file__).resolve().parent
f32 = lambda p: np.fromfile(D / p, "<f4").tolist()

def upload(items):
    st = DeviceState()
    with Device() as dev:
        t = time.time()
        while time.time() - t < 20:
            u = dev.recv(timeout=0.3)
            if u: st.feed(u)
            if st.live_known: break
        for slot, name, data, factor in items:
            dev.import_user_amp(slot, name, data, factor)
            time.sleep(3)
        t = time.time()
        while time.time() - t < 5:
            u = dev.recv(timeout=0.3)
            if u: st.feed(u)
    print("user amps:", st.user_amps[8:])

def reamp(runs):
    subprocess.run([sys.executable, str(HERE / "reamp_run.py"), str(HERE), str(D), json.dumps(runs)], check=True)

if "--analyse" not in sys.argv:
    upload([(8, "SVT-VR Normal n", f32("svtvr_Normal_norm.bin"), 1.0), (9, "SVT-VR Normal n f2", f32("svtvr_Normal_norm.bin"), 2.0)])
    reamp([["DRY", None], ["REF25", 25], ["REF28", 28], ["N1", 38], ["NF2", 39]])
    upload([(9, "SVT-VR Lo n", f32("svtvr_Lo_norm.bin"), 1.0)])
    reamp([["L1", 39]])

# levels: rms over the pink -18 segment and over the -12 dB tones
m = json.load(open(D / "map.json")); segs = m["segments"]; SR = m["sr"]
played = np.load(D / "played.npy").astype(np.float64)
def xcorr_lag(a, b, maxlag):
    n = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, n) * np.conj(np.fft.rfft(a, n)), n)
    c = np.concatenate([c[-maxlag:], c[:maxlag + 1]]); return int(np.argmax(c)) - maxlag
def level(run, names):
    r = np.load(D / f"{run}.npy").astype(np.float64)
    s0 = segs[0]; off = xcorr_lag(played[s0["start"]:s0["start"] + SR], r[:s0["start"] + 3 * SR], 3 * SR - 1)
    v = np.concatenate([r[off + s["start"] + 2400: off + s["start"] + s["len"] - 2400] for s in segs if s["name"] in names])
    return 10 * np.log10(np.mean(v ** 2))
tones = [s["name"] for s in segs if s["name"].startswith("tone") and s["name"].endswith("-12")]
print(f"\n{'run':6} {'pink -18':>9} {'tones -12':>10}   (dBFS rms on the pedal's output)")
res = {}
for run in ("DRY", "REF25", "REF28", "N1", "NF2", "L1"):
    res[run] = (level(run, ["pink -18"]), level(run, tones))
    print(f"{run:6} {res[run][0]:9.1f} {res[run][1]:10.1f}")
print(f"\nfactor 2.0 vs 1.0 (same data): {res['NF2'][0] - res['N1'][0]:+.2f} dB (pink), {res['NF2'][1] - res['N1'][1]:+.2f} dB (tones)")
ref = (res["REF25"][0] + res["REF28"][0]) / 2
for run in ("N1", "L1"):
    print(f"{run} (normalised import) vs the built-in bass amps: {res[run][0] - ref:+.1f} dB (pink)")
print(f"Normal vs Lo after normalisation: {res['N1'][0] - res['L1'][0]:+.1f} dB (pink)")
