import json, sys
import numpy as np
from open_stomp import nam
F = sys.argv[1]; DUMP = sys.argv[2]; FEAT = sys.argv[3]
SR = 48000
BANDS = np.array([31.5*2**(i/3) for i in range(28)])
def bands_db(mag2, fr):
    return np.array([10*np.log10(np.mean(mag2[(fr >= f/2**(1/6)) & (fr < f*2**(1/6))])) for f in BANDS])
def predicted(data):
    g1 = np.array(data[:1024][::-1], dtype=np.float64); g2 = np.array(data[1024:][::-1], dtype=np.float64)
    n = 1 << 16; fr = np.fft.rfftfreq(n, 1/SR)
    return bands_db(np.abs(np.fft.rfft(g1, n) * np.fft.rfft(g2, n))**2, fr)
def norm(v): return v - v[np.argmin(abs(BANDS - 1000))]

official = np.concatenate([np.fromfile(DUMP + "/cb_g1.bin", "<f4"), np.fromfile(DUMP + "/cb_g2.bin", "<f4")])
app = nam.convert(F)
clean = nam.convert(F, measure="clean")
# the real model's small-signal response = the clean H, 1/3-octave
fr = np.arange(4096) * SR / 8192
true = norm(bands_db(10**(np.array(clean.bla_db)/10), fr))
pred = {"official": norm(predicted(official)), "ours (app)": norm(predicted(app.data)), "ours (clean)": norm(predicted(clean.data))}

meas = json.load(open(FEAT))["rec"]["A1"]["sweep -36"]; meas = norm(np.array(meas))
print("validation: predicted official vs measured on the pedal (-36 dBFS sweep):")
d = pred["official"] - meas
print(f"   rms {np.sqrt(np.mean(d**2)):.2f} dB, max {np.max(np.abs(d)):.2f} dB")
show = [0, 3, 6, 7, 9, 12, 15, 18, 21, 24, 27]
print("\nsmall-signal response re 1 kHz (dB):   " + "  ".join(f"{BANDS[i]:>6.0f}" for i in show))
print(f"  {'real model':22}" + "  ".join(f"{true[i]:+6.1f}" for i in show))
for k, v in pred.items():
    print(f"  {k:22}" + "  ".join(f"{v[i]:+6.1f}" for i in show))
print("\nerror vs the real model, rms dB:   all bands | < 125 Hz | 125 Hz-2 kHz | > 2 kHz")
lo, mid, hi = BANDS < 125, (BANDS >= 125) & (BANDS < 2000), BANDS >= 2000
for k, v in pred.items():
    e = v - true; r = lambda sel: np.sqrt(np.mean(e[sel]**2))
    print(f"  {k:22} {r(np.ones_like(e, bool)):9.2f} | {r(lo):8.2f} | {r(mid):12.2f} | {r(hi):7.2f}")
json.dump({"bands": BANDS.tolist(), "true": true.tolist(), "measured_official": meas.tolist(), **{k: v.tolist() for k, v in pred.items()}},
          open(FEAT.replace("nam_features.json", "prediction.json"), "w"))
np.save(FEAT.replace("nam_features.json", "clean_data.npy"), np.array(clean.data, dtype=np.float32))
