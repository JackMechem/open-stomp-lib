import pytest

from open_stomp import Assembler, Module, Msg, Preset, Settings, is_ble_port, is_stomp_port, pack
from open_stomp.catalog import NOTES, drum_pattern_name
from open_stomp.midi import hex, host, req
from open_stomp.records import DEFAULT_CHAIN, PRESET_LEN


def test_pack_roundtrip():
    data = bytes((i * 37 + 11) & 0xFF for i in range(110))
    packed = pack.pack(data)
    assert len(packed) == 126 and all(b < 0x80 for b in packed)
    assert pack.unpack(packed) == data
    assert pack.pack([0, 1, 4])[:3] == bytes([0x00, 0x02, 0x10])


def test_parse():
    m = Msg.parse(b"\xb0\x12\x40")
    assert (m.kind, m.ch, m.cc, m.val) == ("cc", 0, 0x12, 0x40)
    assert Msg.parse(bytearray([0xC1, 4])).prog == 4
    n = Msg.parse([0xF0, 0, 0x21, 0x7A, 0x70, 0x7E, 0x02, 0x19, 0x02, 1, 2, 3, 0, 0, 0x78, 0xF7])
    assert (n.kind, n.what, n.payload) == ("notify", 0x19, bytes([1, 2, 3, 0, 0, 0x78]))
    s = Msg.parse(req.settings())
    assert (s.kind, s.cmd, s.dir, s.payload) == ("nep", 0x14, 0, b"")
    assert s == Msg.parse(req.settings())


def test_assembler_splits_and_joins():
    a = Assembler()
    assert a.push(b"\xf0\x00\x21") == []
    assert a.push(b"\x7a\x00\xf7\xb0\x01\x08\xc0") == [b"\xf0\x00\x21\x7a\x00\xf7", b"\xb0\x01\x08"]
    assert a.push(b"\x05") == [b"\xc0\x05"]


def test_host_commands():
    assert host.head(Module.AMP, 8, False) == bytes([0xB1, 0x01, 0x48])
    assert host.param(Module.OD_FX, 1, 66) == bytes([0xB1, 0x0D, 66])
    assert host.param(Module.GATE, 2, 1) is None
    assert host.head(Module.LOOP, 0, True) is None
    assert host.program(84) == bytes([0xC1, 0x54])
    assert host.page(Module.GATE) == bytes([0xB1, 0x3B, 0x06])
    assert host.looper_button(host.LOOP_REC_PLAY_DUB) == bytes([0xB1, 0x44, 0x01])
    # captured rename (source slot P69 = 0x44)
    assert hex(host.preset_name(0x44, "4Ooz!")) == (
        "F0 00 21 7A 70 12 01 44 34 4F 6F 7A 21 00 00 00 00 00 00 00 00 00 00 00 F7"
    )
    assert hex(req.user_amp(1)) == "F0 00 21 7A 70 58 00 00 02 10 00 00 00 00 00 00 00 00 00 00 00 F7"


def test_catalog():
    assert Module.from_param_cc(0x12) == (Module.AMP, 0)
    assert Module.from_param_cc(0x2B) == (Module.EQ, 11)
    assert Module.from_param_cc(0x3A) is None
    assert len(Module.AMP.models) == 40 and Module.AMP.model_name(8) == "Mar J800"
    assert [p.name for p in Module.AMP.params(0)] == ["GAIN", "BASS", "MIDDLE", "TREBLE", "PRESENCE", "MASTER"]
    mic = Module.CAB.param(3, 0)
    assert (mic.kind, mic.format(4)) == ("enum", "SM57")
    assert Module.EQ.params(0)[0].format(60) == "+10"
    assert drum_pattern_name(13) == "Rock 04" and drum_pattern_name(92) == "Metro 3/4"
    assert len(Module.ALL) == 10 and len(Module.EFFECTS) == 8
    assert Module.from_index(7) == Module.EQ and Module.EQ.index == 7
    assert NOTES[9] == "A"
    assert {Module.AMP: 1}[Module.AMP] == 1  # hashable
    assert is_stomp_port("JME STOMPBOX MIDI 1") and not is_stomp_port("IAC Driver Bus 1")
    assert is_ble_port("JME STOMPBOX (Bluetooth)") and not is_ble_port("JME STOMPBOX MIDI 1")


def test_preset_encode_patches_fields():
    raw = bytearray(PRESET_LEN)
    raw[8] = 0x62
    raw[12] = 0x05  # OD/FX DS1 (3 params)
    raw[25] = 3
    raw[83:90] = bytes(m.index for m in DEFAULT_CHAIN)
    p = Preset.from_raw(raw)
    assert p.to_raw() == bytes(raw)
    p.index = 9
    p.set_module(Module.OD_FX, on=False, model=3)  # Boost RC (4 params)
    p.set_param(Module.OD_FX, 3, 77)
    p.name = "Lead ünicode name too long"
    r = p.to_raw()
    assert (r[0], r[8], r[12], r[25], r[29]) == (9, 0x62, 0x43, 4, 77)
    assert r[94:110] == b"Lead ?nicode nam"
    assert Preset.from_packed(p.to_packed()).to_raw() == r
    with pytest.raises(ValueError):
        Preset.from_raw(b"short")
    with pytest.raises(IndexError):
        p.index = 99
    with pytest.raises(ValueError):
        p.module(Module.LOOP)


def test_modules_are_copies():
    p = Preset()
    p.modules[1].on = True  # changes a copy only
    assert not p.module(Module.AMP).on
    mods = p.modules
    mods[1].on = True
    p.modules = mods
    assert p.module(Module.AMP).on


def test_settings_factory():
    raw = bytes([0x00, 0x0A, 0x1E, 0x0A, 0x32, 0x32, 0x32, 0x32, 0x32, 1, 1, 0x3C, 0xB4, 0, 0, 0, 0])
    s = Settings.from_packed(pack.pack(raw))
    assert s.input_trim_db == 0 and s.eq_mid == 50 and s.wireless and s.loopback
    assert s.to_raw() == raw
    s.input_trim = 13
    s.loopback = False
    r = Settings.from_packed(s.to_packed())
    assert (r.input_trim, r.loopback, r.raw[12]) == (13, False, 0xB4)
    assert host.settings_write(s)[:7] == bytes([0xF0, 0x00, 0x21, 0x7A, 0x70, 0x14, 0x01])
