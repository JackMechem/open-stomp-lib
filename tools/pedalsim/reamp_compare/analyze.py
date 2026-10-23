"""Compare re-amped NAM takes against the reference take; writes data.json and loudness-matched WAVs."""
import json, os, sys
import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfiltfilt, welch, stft

SR = 48000
d, out = sys.argv[1], sys.argv[2]
TAKES = [tuple(t) for t in json.loads(sys.argv[3])] if len(sys.argv) > 3 else [  # key, file, label; the first is the reference
    ("ref", "tone3000_bass_tone_max_session.wav", "TONE3000 (reference)"),
    ("vendor", "vendor-stomp_bass_tone_max_session.wav", "Vendor conversion"),
    ("os", "open-stomp_bass_tone_max_session.wav", "open-stomp conversion"),
]
DI = ("di", sys.argv[4] if len(sys.argv) > 4 else "raw_bass_tone_max.wav", "DI")
CMP = [k for k, _, _ in TAKES if k != "ref"]


def load(f):
    x, sr = sf.read(f if os.path.isabs(f) else os.path.join(d, f))
    assert sr == SR
    return x if x.ndim == 1 else x[:, 0]


di = load(DI[1])
N = len(di)
raw = {k: load(f) for k, f, _ in TAKES}


def envelope(x, band, rate=4000):
    sos = sosfiltfilt  # noqa (keep name short below)
    b = butter(4, band, btype="band", fs=SR, output="sos")
    y = np.abs(sosfiltfilt(b, x))
    y = sosfiltfilt(butter(2, 400, fs=SR, output="sos"), y)
    y = y[:: SR // rate]
    return np.log(np.maximum(y, 0) + 1e-5)


def env_lag(a, b, rate=4000, max_ms=60):
    """lag (samples at SR) of b behind a, from envelope correlation in two bands."""
    lags = []
    for band in ([150, 600], [1000, 4000]):
        ea, eb = envelope(a, band, rate), envelope(b, band, rate)
        n = min(len(ea), len(eb))
        ea, eb = ea[:n] - ea[:n].mean(), eb[:n] - eb[:n].mean()
        M = int(max_ms * rate / 1000)
        c = np.array([np.dot(ea[M:n - M], eb[M + l:n - M + l]) for l in range(-M, M + 1)])
        lags.append(int(np.argmax(c)) - M)
    return int(round(np.mean(lags) * SR / rate)), [l * 1000 / rate for l in lags]


aligned, lag_info = {}, {}
for k, _, _ in TAKES:
    lag, per_band = env_lag(di, raw[k])
    x = raw[k][lag:] if lag >= 0 else np.concatenate([np.zeros(-lag), raw[k]])
    x = x[:N]
    aligned[k] = np.concatenate([x, np.zeros(N - len(x))]) if len(x) < N else x
    lag_info[k] = {"ms": lag * 1000 / SR, "per_band_ms": per_band}

db = lambda v: 20 * np.log10(np.maximum(v, 1e-12))
rms = lambda x: float(np.sqrt(np.mean(x ** 2)))

# loudness match to the reference (RMS over the playing part: frames above -50 dB re the loudest)
def active_rms(x, ref_mask):
    return float(np.sqrt(np.mean(x[ref_mask] ** 2)))

fr = 2400  # 50 ms
nfr = N // fr
frame_rms = lambda x: np.sqrt(np.mean(x[: nfr * fr].reshape(nfr, fr) ** 2, axis=1))
ref_fr = frame_rms(aligned["ref"])
mask_fr = db(ref_fr) > db(ref_fr.max()) - 50
mask = np.repeat(mask_fr, fr)
mask = np.concatenate([mask, np.zeros(N - len(mask), bool)])
gain = {k: active_rms(aligned["ref"], mask) / active_rms(aligned[k], mask) for k, _, _ in TAKES}
matched = {k: aligned[k] * gain[k] for k, _, _ in TAKES}


def clip_events(x, thr=0.999):
    m = np.abs(x) >= thr
    starts = np.flatnonzero(m[1:] & ~m[:-1]) + 1
    if m[0]:
        starts = np.r_[0, starts]
    return [round(s / SR, 3) for s in starts], int(m.sum())


summary = {}
for k, f, label in [DI] + TAKES:
    x = di if k == "di" else raw[k]
    ev, n = clip_events(x)
    summary[k] = {
        "label": label, "file": f,
        "rms_dbfs": round(db(rms(x[:N])), 2),
        "peak_dbfs": round(db(np.abs(x).max()), 2),
        "crest_db": round(db(np.abs(x[:N]).max()) - db(rms(x[:N])), 2),
        "clip_samples": n, "clip_events": ev,
    }
    if k != "di":
        summary[k]["offset_ms"] = round(lag_info[k]["ms"], 2)
        summary[k]["gain_db"] = round(db(gain[k]), 2)

# ---- long-term spectrum, 1/6-octave smoothed ----
f, P = None, {}
for k, _, _ in TAKES:
    f, p = welch(matched[k][mask], SR, nperseg=16384)
    P[k] = p
fdi, pdi = welch(di[mask], SR, nperseg=16384)
centres = 25 * 2 ** (np.arange(0, 6 * np.log2(18000 / 25) + 1) / 6)
def smooth(p):
    o = []
    for c in centres:
        lo, hi = c * 2 ** (-1 / 12), c * 2 ** (1 / 12)
        sel = (f >= lo) & (f < hi)
        o.append(10 * np.log10(p[sel].mean()) if sel.any() else np.nan)
    return np.array(o)
S = {k: smooth(P[k]) for k in P}
norm = np.nanmax(S["ref"])
ltas = {k: np.round(S[k] - norm, 2).tolist() for k in S}
sdi = smooth(pdi); ltas["di"] = np.round(sdi - np.nanmax(sdi), 2).tolist()
ltas_diff = {k: np.round(S[k] - S["ref"], 2).tolist() for k in CMP}
band_sel = (centres >= 40) & (centres <= 10000)
ltas_err = {k: round(float(np.sqrt(np.mean((S[k] - S["ref"])[band_sel] ** 2))), 2) for k in CMP}

# ---- third-octave band levels over time ----
hop = 2400  # 50 ms frames, 100 ms windows
fz, tz, Z = {}, None, {}
for k, _, _ in TAKES:
    fz, tz, z = stft(matched[k], SR, nperseg=4800, noverlap=4800 - hop, boundary=None, padded=False)
    Z[k] = np.abs(z) ** 2
_, _, zdi = stft(di, SR, nperseg=4800, noverlap=4800 - hop, boundary=None, padded=False)
third = 25 * 2 ** (np.arange(0, 3 * np.log2(12500 / 25) + 1) / 3)  # 25 Hz .. 12.5 kHz
def bands(z):
    rows = []
    for c in third:
        sel = (fz >= c * 2 ** (-1 / 6)) & (fz < c * 2 ** (1 / 6))
        rows.append(z[sel].sum(axis=0))
    return 10 * np.log10(np.array(rows) + 1e-20)
B = {k: bands(Z[k]) for k in Z}
nb, nt = B["ref"].shape
ref_frame_max = B["ref"].max(axis=0)
loud = ref_frame_max > ref_frame_max.max() - 45           # frames where something is playing
valid = (B["ref"] > ref_frame_max[None, :] - 40) & loud[None, :] & (third >= 40)[:, None]
err_frames = {}
for k in CMP:
    dlt = B[k] - B["ref"]
    e = np.sqrt(np.nanmean(np.where(valid, dlt ** 2, np.nan), axis=0))
    err_frames[k] = e
# DI level per frame
di_lvl = 10 * np.log10(zdi.__abs__().__pow__(2).sum(axis=0) + 1e-20)
di_lvl -= di_lvl.max()

# downsample time series to 0.5 s for the line chart (mean of valid frames)
step = 10
def pool(v, how=np.nanmean):
    m = len(v) // step
    with np.errstate(all="ignore"):
        return [None if np.isnan(a) else round(float(a), 2) for a in how(v[: m * step].reshape(m, step), axis=1)]
t_pool = (np.arange(len(err_frames[CMP[0]]) // step) * step + step / 2) * hop / SR
err_series = {k: pool(err_frames[k]) for k in err_frames}
di_series = pool(np.where(loud, di_lvl, np.nan), np.nanmax)
err_mean = {k: round(float(np.nanmean(err_frames[k])), 2) for k in err_frames}
err_median = {k: round(float(np.nanmedian(err_frames[k])), 2) for k in err_frames}
# for each take, the share of moments where it is the closest of all
stack = np.array([err_frames[k] for k in CMP]); okm = ~np.isnan(stack).any(axis=0)
best = np.argmin(np.where(np.isnan(stack), np.inf, stack), axis=0)
closest = {k: int(np.sum(okm & (best == i))) for i, k in enumerate(CMP)}
both = int(okm.sum()); wins = 0

# heatmap: per band difference (dB), pooled to 0.25 s
hstep = 5
m = nt // hstep
heat = {}
for k in CMP:
    dlt = np.where(valid, B[k] - B["ref"], np.nan)[:, : m * hstep].reshape(nb, m, hstep)
    with np.errstate(all="ignore"):
        h = np.nanmean(dlt, axis=2)
    heat[k] = [[None if np.isnan(v) else round(float(v), 1) for v in row] for row in h]
# reference band levels (context heatmap)
refh = B["ref"][:, : m * hstep].reshape(nb, m, hstep).mean(axis=2)
refh = refh - refh.max()
heat["ref"] = [[round(float(max(v, -60)), 1) for v in row] for row in refh]

# per-band mean difference (where valid) and mean absolute difference
band_bias = {k: [None if np.isnan(v) else round(float(v), 2) for v in
                 np.nanmean(np.where(valid, B[k] - B["ref"], np.nan), axis=1)] for k in CMP}
band_abs = {k: [None if np.isnan(v) else round(float(v), 2) for v in
                np.nanmean(np.where(valid, np.abs(B[k] - B["ref"]), np.nan), axis=1)] for k in CMP}

# ---- dynamics: output level vs DI input level (50 ms frames) ----
di_fr = db(frame_rms(di))
out_fr = {k: db(frame_rms(matched[k])) for k in matched}
act = di_fr > di_fr.max() - 45
edges = np.arange(np.floor(di_fr[act].min()), np.ceil(di_fr.max()) + 2, 2)
dyn = {"x": [], **{k: [] for k in matched}, "n": []}
for lo, hi in zip(edges[:-1], edges[1:]):
    sel = act & (di_fr >= lo) & (di_fr < hi)
    if sel.sum() < 15:
        continue
    dyn["x"].append(round(float((lo + hi) / 2), 1))
    dyn["n"].append(int(sel.sum()))
    for k in matched:
        dyn[k].append(round(float(np.median(out_fr[k][sel])), 2))

# ---- overview envelope for the player (max abs per 0.1 s) ----
ov = 4800
mo = N // ov
overview = {k: np.round(np.abs(v[: mo * ov]).reshape(mo, ov).max(axis=1), 3).tolist()
            for k, v in [("di", di)] + list(matched.items())}

# ---- loudness-matched, aligned WAVs for listening ----
os.makedirs(os.path.join(d, "compare"), exist_ok=True)
di_matched = di * active_rms(aligned["ref"], mask) / active_rms(di, mask)
peak = max(np.abs(v).max() for v in list(matched.values()) + [di_matched])
head = 0.97 / peak
for k, v in list(matched.items()) + [("di", di_matched)]:
    sf.write(os.path.join(d, "compare", f"{k}.wav"), (v * head).astype(np.float32), SR, subtype="PCM_24")

json.dump({
    "duration": N / SR, "summary": summary,
    "listen_trim_db": round(db(head), 2),
    "centres": np.round(centres, 1).tolist(), "ltas": ltas, "ltas_diff": ltas_diff, "ltas_err": ltas_err,
    "third": np.round(third, 1).tolist(), "band_bias": band_bias, "band_abs": band_abs,
    "t_pool": np.round(t_pool, 2).tolist(), "err_series": err_series, "di_series": di_series,
    "err_mean": err_mean, "err_median": err_median, "frames_os_better": wins, "frames_compared": both,
    "closest": closest, "takes": [[k, f, l] for k, f, l in TAKES], "cmp": CMP,
    "low_err": {k: round(float(np.sqrt(np.mean((S[k] - S["ref"])[(centres >= 40) & (centres <= 160)] ** 2))), 2) for k in CMP},
    "heat_dt": hstep * hop / SR, "heat": heat,
    "dyn": dyn, "overview_dt": ov / SR, "overview": overview,
}, open(out, "w"), separators=(",", ":"))
print(json.dumps({k: summary[k] for k in summary}, indent=1))
print("ltas_err", ltas_err, "median", err_median, "closest", closest, "/", both)
print("trim", db(head))
