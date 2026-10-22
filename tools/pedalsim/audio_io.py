"""Play on one device while recording another (two independent streams)."""
import threading
import numpy as np
import sounddevice as sd

SR = 48000

def find(name):
    for i, d in enumerate(sd.query_devices()):
        if name in d["name"]:
            return i
    raise SystemExit(f"audio device {name!r} not found")

def playrec(signal, out_dev, in_dev, out_channel=0, in_channels=2, pre=0.3, post=0.5):
    """Play mono `signal` on out_dev's channel `out_channel`, recording in_dev from `pre` s before
    to `post` s after. Returns the recording (frames × in_channels, float32)."""
    total = int((pre + len(signal) / SR + post) * SR)
    rec = np.zeros((total, in_channels), np.float32)
    pos = [0]
    done = threading.Event()

    def in_cb(data, frames, t, status):
        n = min(frames, total - pos[0])
        rec[pos[0]:pos[0] + n] = data[:n]
        pos[0] += n
        if pos[0] >= total:
            done.set()
            raise sd.CallbackStop

    out = np.zeros((len(signal), 2), np.float32)
    out[:, out_channel] = signal
    with sd.InputStream(device=in_dev, channels=in_channels, samplerate=SR, dtype="float32", callback=in_cb):
        sd.sleep(int(pre * 1000))
        sd.play(out, SR, device=out_dev, blocking=True)
        done.wait(timeout=len(signal) / SR + pre + post + 5)
    return rec
