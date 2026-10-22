"""Re-amped IR uploads against the full IR in software: 1/3-octave response of what the pedal played
(the CAB block's measured response, `rel_<RUN>_<level>.npy` from ir_reamp_analyze.py) vs the IR at
the level the official app means to give it.
usage: ir_reamp_compare.py <dir> <ir.wav> RUN [RUN ...]
"""
import sys
from pathlib import Path
import numpy as np
from open_stomp import ir

D, wav, runs = Path(sys.argv[1]), sys.argv[2], sys.argv[3:]
BANDS = 31.5 * 2 ** (np.arange(28) / 3)

def third_octaves(h, n=1 << 16):
    H = np.abs(np.fft.rfft(h, n)) ** 2; f = np.fft.rfftfreq(n, 1 / 48000)
    return np.array([10 * np.log10(np.mean(H[(f >= b / 2 ** (1 / 6)) & (f < b * 2 ** (1 / 6))]) + 1e-30) for b in BANDS])

rate, ch = ir.load_wav(wav)
x = np.array(ir.resample(ch[0], rate, 48000), float)
onset = max(0, int(np.argmax(np.abs(x) > 0.01 * np.abs(x).max())) - 8)
ref = third_octaves(x)   # the full IR as filed; "level" compares runs with each other
print(f"{Path(wav).stem}: onset at sample {onset}")
print(f"{'run':8} {'shape':>6} {'level':>6}   1/3-octave difference (dB, level removed) at " + " ".join(f"{b:.0f}" for b in BANDS[::3]))
for r in runs:
    for lvl in ("-24", "-12"):
        rel = np.load(D / f"rel_{r}_{lvl}.npy")
        d = third_octaves(rel) - ref
        m = d.mean()
        print(f"{r + ' ' + lvl:8} {np.sqrt(np.mean((d - m) ** 2)):6.2f} {m:+6.1f}   " + " ".join(f"{v:+.1f}" for v in (d - m)[::3]))
