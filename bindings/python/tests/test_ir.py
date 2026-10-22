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
    # reversed, cut to 2048 samples, unit energy
    assert c.data[2047] == pytest.approx(0.6) and c.data[2037] == pytest.approx(-0.8)
    assert c.gain == pytest.approx(2.0) and len(c.ir) == 3000


def test_resample_length_and_errors(tmp_path):
    assert len(ir.resample([0.0] * 441, 44100, 48000)) == 480
    with pytest.raises(OSError):
        ir.load_wav(tmp_path / "missing.wav")
    bad = tmp_path / "bad.wav"
    bad.write_bytes(b"not a wav")
    with pytest.raises(ValueError):
        ir.convert(bad)
