"""IR files from Python: WAV round trip, resampling, conversion."""

import pytest

from open_stomp import ir


def test_wav_round_trip_and_convert(tmp_path):
    p = tmp_path / "Cab Test IR With A Long Name.wav"
    x = [0.0] * 3000
    x[0], x[10], x[2500] = 0.3, -0.4, 0.9
    ir.save_wav(p, 48000, [x], float=True)
    rate, channels = ir.load_wav(p)
    assert rate == 48000 and channels == [pytest.approx(x)]
    c = ir.convert(p)
    assert len(c.data) == ir.IR_TAPS == 2048
    # reversed, cut to 2048 samples, scaled like the official app
    assert c.data[2037] / c.data[2047] == pytest.approx(-0.4 / 0.3, rel=1e-5)
    assert c.data[2047] == pytest.approx(0.3 * c.gain, rel=1e-5) and len(c.ir) == 3000


def test_resample_length_and_errors(tmp_path):
    assert len(ir.resample([0.0] * 441, 44100, 48000)) == 480
    with pytest.raises(OSError):
        ir.load_wav(tmp_path / "missing.wav")
    bad = tmp_path / "bad.wav"
    bad.write_bytes(b"not a wav")
    with pytest.raises(ValueError):
        ir.convert(bad)


def test_optimised_conversion(tmp_path):
    # a resonance starting 1000 samples in: the optimised upload keeps it within the 384 played taps
    import math
    x = [0.0] * 1000 + [math.exp(-n / 120) * math.sin(n * 0.05) for n in range(5000)]
    x[1000] = 0.5
    p = tmp_path / "late.wav"
    ir.save_wav(p, 48000, [x], float=True)
    c = ir.convert(p, method="optimised")
    assert len(c.data) == 2048 and all(v == 0.0 for v in c.data[: 2048 - ir.PEDAL_TAPS])
    played = sum(v * v for v in c.data[2048 - ir.PEDAL_TAPS :])
    official = ir.convert(p)
    assert played > 1000 * sum(v * v for v in official.data[2048 - ir.PEDAL_TAPS :])
    with pytest.raises(ValueError):
        ir.convert(p, method="best")
