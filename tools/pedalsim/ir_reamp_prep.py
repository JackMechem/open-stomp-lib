"""Upload the IR re-amp test material (pedal connected, official app closed):
USER IR 10 = a probe (impulses at known taps and levels), USER IR 09 = a real IR through ir.convert.
usage: ir_reamp_prep.py <out dir> [real_ir.wav]
"""
import json, sys, time
from pathlib import Path
import numpy as np
from open_stomp import Device, DeviceState, ir

OUT = Path(sys.argv[1]); OUT.mkdir(parents=True, exist_ok=True)
# probe: tap k = 0 … 2047 of the IR; impulses with distinct signs/levels so each can be identified
PROBE = {0: 1.0, 500: 0.5, 1000: -0.5, 1500: 0.25, 2040: -0.25}
h = np.zeros(ir.IR_TAPS)
for k, v in PROBE.items():
    h[k] = v
probe_data = h[::-1].astype(np.float32)          # uploaded reversed
uploads = [(9, "IR probe", probe_data)]
if len(sys.argv) > 2:
    c = ir.convert(sys.argv[2])
    uploads.append((8, Path(sys.argv[2]).stem[:16], np.array(c.data, np.float32)))   # named like the app
    np.save(OUT / "real_ir_48k.npy", np.array(c.ir, np.float32))
np.save(OUT / "probe_taps.npy", h)
st = DeviceState()
with Device() as dev:
    t = time.time()
    while time.time() - t < 20:
        u = dev.recv(timeout=0.3)
        if u: st.feed(u)
        if st.live_known: break
    for slot, name, data in uploads:
        dev.import_user_ir(slot, name, data.tolist())
        np.save(OUT / f"ir_slot{slot + 1:02d}.npy", data)
        time.sleep(3)
    t = time.time()
    while time.time() - t < 6:
        u = dev.recv(timeout=0.3)
        if u: st.feed(u)
print("user IR slots now:", st.user_irs)
json.dump({"probe": {str(k): v for k, v in PROBE.items()}, "uploads": [[s + 1, n] for s, n, _ in uploads]}, open(OUT / "ir_prep.json", "w"), indent=1)
