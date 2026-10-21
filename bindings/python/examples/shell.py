"""Interactive shell for the pedal: inspect state, send live edits, save presets and settings.

    python examples/shell.py                          # auto-connect, sync, then prompt
    python examples/shell.py --listen                 # send nothing at all
    python examples/shell.py --replay ../../captures/logs/01-main-session.log
    python examples/shell.py -c status -c "show live" # run commands after the sync, then exit

Type `help` at the prompt for the commands. Preset and settings writes ask for confirmation.
"""

import argparse
import cmd
import json
import shlex
import sys
import threading
import time
from typing import Optional

import open_stomp
from open_stomp import Device, DeviceState, Module, Origin, Preset, Update, capture, catalog, is_stomp_port, midi, pack
from open_stomp.midi import host
from open_stomp.records import PRESET_COUNT, preset_label

MODULE_ALIASES = {
    "od": Module.OD_FX, "fx": Module.OD_FX, "odfx": Module.OD_FX, "od/fx": Module.OD_FX,
    "amp": Module.AMP, "cab": Module.CAB, "mod": Module.MOD,
    "dly": Module.DELAY, "delay": Module.DELAY, "rvb": Module.REVERB, "reverb": Module.REVERB,
    "gate": Module.GATE, "eq": Module.EQ, "loop": Module.LOOP, "drum": Module.DRUM,
}
LOOP_BUTTONS = {
    "rec": host.LOOP_REC_PLAY_DUB, "play": host.LOOP_REC_PLAY_DUB, "dub": host.LOOP_REC_PLAY_DUB,
    "stop": host.LOOP_STOP, "clear": host.LOOP_CLEAR, "undo": host.LOOP_UNDO_REDO, "redo": host.LOOP_UNDO_REDO,
}


class UsageError(Exception):
    pass


def parse_module(s: str) -> Module:
    m = MODULE_ALIASES.get(s.lower())
    if m is None:
        raise UsageError(f"unknown module {s!r} (one of: {', '.join(sorted(MODULE_ALIASES))})")
    return m


def parse_preset(s: str) -> int:
    """'5' / 'P05' / 'p5' -> 4 (0-based)."""
    n = int(s[1:] if s[:1] in "pP" else s)
    if not 1 <= n <= PRESET_COUNT:
        raise UsageError(f"preset must be 1-{PRESET_COUNT}")
    return n - 1


def parse_bool(s: str) -> bool:
    if s.lower() in ("on", "1", "true", "yes"):
        return True
    if s.lower() in ("off", "0", "false", "no"):
        return False
    raise UsageError(f"expected on/off, got {s!r}")


def parse_hex(args: list[str]) -> bytes:
    return bytes.fromhex("".join(args).replace(",", ""))


def clock(at) -> str:
    t = at or time.time()
    return time.strftime("%H:%M:%S", time.localtime(t)) + f".{int(t * 1000) % 1000:03d}"


class Engine:
    """Owns the Device; a background thread feeds every update into a DeviceState.

    The shell reads the same DeviceState from the main thread, so every access goes through ``lock``.
    """

    def __init__(self, args: argparse.Namespace) -> None:
        self.state = DeviceState()
        self.lock = threading.Lock()
        self.synced = threading.Event()
        self.ports: list[str] = []
        self.status: Optional[str] = "disconnected"
        self.port: Optional[str] = None
        self.sync_progress = (0, 0)
        self.monitor = True
        self.show_all = False
        self.show_raw = False
        self.dev = Device(
            sync=not args.listen,
            auto_connect=not (args.port or args.replay),
            reply_timeout=args.timeout,
        )
        if args.port:
            self.dev.connect(args.port)
        if args.replay:
            self.dev.replay(args.replay, args.speed)
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def log(self, text: str) -> None:
        # print above the prompt without wrecking it too much
        sys.stdout.write(f"\r\x1b[K{text}\n")
        sys.stdout.flush()

    def _run(self) -> None:
        try:
            for u in self.dev:
                self._handle(u)
        except Exception as e:  # noqa: BLE001
            self.log(f"!! engine thread died: {e!r}")

    def _handle(self, u: Update) -> None:
        if u.kind == "ports":
            self.ports = u.ports or []
            if self.monitor and not any(is_stomp_port(p) for p in self.ports):
                self.log(f"-- waiting for the pedal (MIDI inputs: {self.ports})")
        elif u.kind == "status":
            self.status, self.port = u.status, u.port
            if u.status == "replaying" and u.progress == 1.0:  # a replay has no sync; its end counts as one
                self.synced.set()
            if self.monitor:
                extra = "" if u.output in (None, True) else " (input only)"
                self.log(f"-- status: {u.status} {u.port or u.file or ''}{extra}")
        elif u.kind == "sync":
            self.sync_progress = (u.done or 0, u.total or 0)
            if u.total == 0 and self.state.firmware and not self.synced.is_set():
                self.synced.set()
                if self.monitor:
                    with self.lock:
                        cur = self.state.current
                        name = self.state.preset_name(cur) if cur is not None else None
                        self.log(f"-- synced: {self.state.product} fw {self.state.firmware}, active "
                                 f"{preset_label(cur) if cur is not None else '?'} {name or ''}")
        elif u.kind == "reset":
            self.synced.clear()
        elif u.kind == "mark" and self.monitor:
            self.log(f"{clock(u.at)} ===== {u.text} =====")
        elif u.kind == "info" and (self.monitor or u.error):
            self.log(f"{clock(u.at)} {'!! ' if u.error else '-- '}{u.text}")

        with self.lock:
            ev = self.state.feed(u)
        if ev is not None and self.monitor:
            if (ev.category == "protocol" or ev.redundant) and not self.show_all:
                return
            who = "PEDAL" if u.origin == Origin.PEDAL else "HOST "
            raw = f"  [{midi.hex(ev.raw)}]" if self.show_raw else ""
            self.log(f"{clock(u.at)} {who} {ev.label:8} {ev.subject:40} {ev.value}{raw}")

    def close(self) -> None:
        self.dev.close()


def describe_preset(p: Preset, state: DeviceState) -> str:
    lines = [f"{preset_label(p.index)} {p.name!r}  vol={p.volume}  "
             f"chain=GATE > {' > '.join(m.label for m in p.chain)}"]
    for m in [Module.GATE, *p.chain]:
        ms = p.module(m)
        name = state.model_name(m, ms.model)
        defs = m.params(ms.model)
        vals = ", ".join(f"{d.name}={d.format(ms.params[d.slot])}" for d in defs if d.slot < len(ms.params))
        lines.append(f"  {m.label:7} {'ON ' if ms.on else 'off'} #{ms.model + 1:02} {name:20} {vals}")
    return "\n".join(lines)


class Shell(cmd.Cmd):
    intro = "open_stomp shell — `help` for commands, Ctrl-D to quit"
    prompt = "stomp> "

    def __init__(self, eng: Engine):
        super().__init__()
        self.eng = eng
        self.dev = eng.dev
        self.state = eng.state

    # ---- plumbing ----------------------------------------------------------------------------
    def onecmd(self, line):
        try:
            return super().onecmd(line)
        except UsageError as e:
            print(f"usage: {e}")
        except (ValueError, IndexError, RuntimeError, OSError) as e:
            print(f"{type(e).__name__}: {e}")
        return False

    def emptyline(self):
        return False

    def do_EOF(self, _):
        print()
        return True

    def do_quit(self, _):
        """quit — exit"""
        return True

    do_exit = do_quit

    # ---- connection --------------------------------------------------------------------------
    def do_status(self, _):
        """status — connection, sync progress, device identity"""
        e = self.eng
        print(f"status:   {e.status} {e.port or ''}")
        print(f"ports:    {e.ports}")
        print(f"sync:     {e.sync_progress[0]}/{e.sync_progress[1]}  synced={e.synced.is_set()}")
        with e.lock:
            s = self.state
            print(f"product:  {s.product}  model_id={s.model_id}  firmware={s.firmware}")
            cur = s.current
            print(f"current:  {preset_label(cur) if cur is not None else None}  page={s.page.label if s.page else None}"
                  f"  live_known={s.live_known}  modified={s.modified()}")
            print(f"user amps: {s.user_amps}")
            print(f"user IRs:  {s.user_irs}")
        print(f"lib:      open_stomp {open_stomp.__version__}  USB {'%04x:%04x' % open_stomp.USB_ID}")

    def do_ports(self, _):
        """ports — list MIDI input ports seen by the engine"""
        for p in self.eng.ports:
            print(("* " if is_stomp_port(p) else "  ") + p)

    def do_connect(self, arg):
        """connect PORT — connect to a named MIDI input port"""
        if not arg:
            raise UsageError("connect PORT")
        self.dev.connect(arg)

    def do_disconnect(self, _):
        """disconnect"""
        self.dev.disconnect()

    def do_resync(self, _):
        """resync — re-run the startup sync"""
        self.dev.resync()

    def do_listen(self, arg):
        """listen on|off — on = never send anything (set_sync(False))"""
        self.dev.set_sync(not parse_bool(arg or "on"))

    def do_replay(self, arg):
        """replay FILE [SPEED] — replay a capture log through the engine"""
        a = shlex.split(arg)
        if not a:
            raise UsageError("replay FILE [SPEED]")
        self.dev.replay(a[0], float(a[1]) if len(a) > 1 else 4.0)

    def do_stopreplay(self, _):
        """stopreplay"""
        self.dev.stop_replay()

    def do_wait(self, arg):
        """wait [SECONDS] — block until the startup sync finishes (default 15 s)"""
        t = float(arg or 15)
        print("synced" if self.eng.synced.wait(t) else f"not synced after {t}s")

    def do_sleep(self, arg):
        """sleep SECONDS — useful in -c scripts"""
        time.sleep(float(arg or 1))

    # ---- output ------------------------------------------------------------------------------
    def do_monitor(self, arg):
        """monitor on|off|all|raw — live event printing (all = include protocol/redundant, raw = show bytes)"""
        a = (arg or "on").lower()
        if a == "all":
            self.eng.monitor = self.eng.show_all = True
        elif a == "raw":
            self.eng.monitor = True
            self.eng.show_raw = not self.eng.show_raw
            print(f"raw bytes {'on' if self.eng.show_raw else 'off'}")
        else:
            self.eng.monitor = parse_bool(a)
            if not self.eng.monitor:
                self.eng.show_all = False

    # ---- state -------------------------------------------------------------------------------
    def do_presets(self, _):
        """presets — list all 99 preset names (* = active)"""
        with self.eng.lock:
            cur = self.state.current
            names = [self.state.preset_name(i) for i in range(PRESET_COUNT)]
        for i, n in enumerate(names):
            print(f"{'*' if i == cur else ' '}{preset_label(i)} {n if n is not None else '<not read>'}")

    def do_show(self, arg):
        """show [live|N] — dump a preset (default: the live edit buffer)"""
        with self.eng.lock:
            p: Optional[Preset]
            if not arg or arg == "live":
                p = self.state.live
                if not self.state.live_known:
                    print("(edit buffer not read yet)")
            else:
                p = self.state.stored(parse_preset(arg))
                if p is None:
                    print("not read yet")
                    return
            print(describe_preset(p, self.state))

    def do_raw(self, arg):
        """raw [live|N] — hex of a preset's raw 110-byte record"""
        with self.eng.lock:
            p = self.state.live if not arg or arg == "live" else self.state.stored(parse_preset(arg))
        if p is None:
            print("not read yet")
            return
        print(midi.hex(p.to_raw()))

    def do_settings(self, _):
        """settings — show system settings"""
        with self.eng.lock:
            s = self.state.settings
        if s is None:
            print("settings not read yet")
            return
        print(f"input_trim={s.input_trim} ({s.input_trim_db:+d} dB)  eq bass={s.eq_bass} mid={s.eq_mid} "
              f"treble={s.eq_treble}  wireless={s.wireless}  loopback={s.loopback}")
        print(f"raw: {midi.hex(s.raw)}")

    def do_looper(self, _):
        """looper — looper / drum / tuner state"""
        with self.eng.lock:
            lp, dr, tu = self.state.looper, self.state.drum, self.state.tuner
        print(f"looper: {lp.state_name} (state={lp.state}) layers={lp.layers} rec={lp.rec_level} "
              f"play={lp.play_level} sync={lp.sync} mode={lp.mode}")
        pat = catalog.drum_pattern_name(dr.pattern) if dr.pattern is not None else None
        print(f"drum:   playing={dr.playing} pattern={dr.pattern} ({pat}) level={dr.level} bpm={dr.bpm}")
        note = catalog.NOTES[tu.note] if tu.note is not None and tu.note < len(catalog.NOTES) else tu.note
        print(f"tuner:  active={tu.active} detected={tu.detected} note={note} cents={tu.cents} "
              f"offset={tu.cents_offset}")

    do_drum = do_tuner = do_looper

    def do_backup(self, arg):
        """backup FILE — write every read preset (packed hex) + settings to a JSON file"""
        if not arg:
            raise UsageError("backup FILE")
        with self.eng.lock:
            presets = {preset_label(i): {"name": p.name, "packed": p.to_packed().hex()}
                       for i, p in enumerate(self.state.presets) if p is not None}
            settings = self.state.settings
            out = {
                "firmware": self.state.firmware,
                "presets": presets,
                "settings": settings.to_packed().hex() if settings else None,
            }
        with open(arg, "w") as f:
            json.dump(out, f, indent=1)
        print(f"wrote {len(presets)} presets to {arg}")

    # ---- catalogue ---------------------------------------------------------------------------
    def do_models(self, arg):
        """models MODULE — list a module's models"""
        m = parse_module(arg or "amp")
        for i, n in enumerate(m.models):
            print(f"#{i + 1:02} (model={i}) {n}")

    def do_params(self, arg):
        """params MODULE [MODEL#] — list parameters (MODEL# is 1-based like the pedal shows)"""
        a = arg.split()
        if not a:
            raise UsageError("params MODULE [MODEL#]")
        m = parse_module(a[0])
        model = int(a[1]) - 1 if len(a) > 1 else self.state.live.module(m).model
        print(f"{m.label} #{model + 1:02} {m.model_name(model)}  head_cc={m.head_cc} param_ccs={m.param_ccs}")
        for d in m.params(model):
            extra = f" choices={d.choices}" if d.choices else ""
            print(f"  slot {d.slot}: {d.name:12} {d.kind}{extra}")

    # ---- live control ------------------------------------------------------------------------
    def do_select(self, arg):
        """select N — select preset N (1-99 or P05)"""
        self.dev.select_preset(parse_preset(arg))

    def do_head(self, arg):
        """head MODULE MODEL#|- on|off — set model (1-based, '-' = keep current) and on/off"""
        a = arg.split()
        if len(a) != 3:
            raise UsageError("head MODULE MODEL#|- on|off")
        m = parse_module(a[0])
        with self.eng.lock:
            model = self.state.live.module(m).model if a[1] == "-" else int(a[1]) - 1
        self.dev.set_head(m, model, parse_bool(a[2]))

    def do_on(self, arg):
        """on MODULE — switch a block on, keeping its model"""
        self.do_head(f"{arg} - on")

    def do_off(self, arg):
        """off MODULE — switch a block off, keeping its model"""
        self.do_head(f"{arg} - off")

    def do_param(self, arg):
        """param MODULE SLOT|NAME VALUE — set a parameter (e.g. `param amp gain 80`)"""
        a = arg.split()
        if len(a) != 3:
            raise UsageError("param MODULE SLOT|NAME VALUE")
        m = parse_module(a[0])
        if a[1].isdigit():
            slot = int(a[1])
        else:
            with self.eng.lock:
                model = self.state.live.module(m).model
            matches = [d.slot for d in m.params(model) if d.name.lower() == a[1].lower()]
            if not matches:
                raise UsageError(f"no param {a[1]!r} on {m.label} #{model + 1}: "
                                 f"{[d.name for d in m.params(model)]}")
            slot = matches[0]
        self.dev.set_param(m, slot, int(a[2]))

    def do_vol(self, arg):
        """vol VALUE — preset volume"""
        self.dev.set_volume(int(arg))

    def do_page(self, arg):
        """page MODULE — make the pedal display jump to a module page"""
        self.dev.show_page(parse_module(arg))

    def do_loop(self, arg):
        """loop rec|play|dub|stop|clear|undo|redo — press a looper button"""
        b = LOOP_BUTTONS.get(arg.lower())
        if b is None:
            raise UsageError(f"loop {'|'.join(LOOP_BUTTONS)}")
        self.dev.looper_button(b)

    def do_cc(self, arg):
        """cc CC VAL — send a raw CC on the host channel (numbers accept 0x..)"""
        a = arg.split()
        if len(a) != 2:
            raise UsageError("cc CC VAL")
        self.dev.send(host.cc(int(a[0], 0), int(a[1], 0)))

    def do_send(self, arg):
        """send HEX... — send raw bytes, e.g. `send F0 00 21 7A 70 14 00 F7`"""
        self.dev.send(parse_hex(arg.split()))

    def do_req(self, arg):
        """req NAME [ARG] — send a read request: version product settings cur_state drum looper tuner
        edit_buffer ir_slot N, preset N (1-based), user_amp N"""
        a = arg.split()
        if not a or not hasattr(midi.req, a[0]):
            raise UsageError("req version|product|settings|cur_state|drum|looper|tuner|edit_buffer|"
                             "ir_slot N|preset N|user_amp N")
        fn = getattr(midi.req, a[0])
        if a[0] == "preset":
            data = fn(parse_preset(a[1]))
        elif len(a) > 1:
            data = fn(int(a[1]))
        else:
            data = fn()
        print(f"-> {midi.hex(data)}")
        self.dev.send(data)

    # ---- writes (confirm) --------------------------------------------------------------------
    def _confirm(self, what: str) -> bool:
        if not sys.stdin.isatty():
            print(f"refusing to {what} without a tty to confirm")
            return False
        return input(f"{what} — type 'yes' to confirm: ").strip() == "yes"

    def do_save(self, arg):
        """save DEST [NAME] — save the live edit buffer to slot DEST (asks for confirmation)"""
        a = shlex.split(arg)
        if not a:
            raise UsageError("save DEST [NAME]")
        with self.eng.lock:
            if not self.state.live_known or self.state.current is None:
                print("edit buffer / current preset not known yet")
                return
            p, src = self.state.live, self.state.current
        p.index = parse_preset(a[0])
        if len(a) > 1:
            p.name = a[1]
        msgs = host.save_preset(src, p)
        print(f"will send {len(msgs)} messages (source {preset_label(src)} -> {preset_label(p.index)} {p.name!r}):")
        for m in msgs:
            print(f"  {midi.hex(m)[:100]}{'…' if len(midi.hex(m)) > 100 else ''}")
        if self._confirm(f"overwrite {preset_label(p.index)}"):
            self.dev.save_preset(src, p)

    def do_set(self, arg):
        """set FIELD VALUE — change one system setting and write it (input_trim eq_bass eq_mid eq_treble
        wireless loopback; asks for confirmation)"""
        a = arg.split()
        fields = ("input_trim", "eq_bass", "eq_mid", "eq_treble", "wireless", "loopback")
        if len(a) != 2 or a[0] not in fields:
            raise UsageError(f"set {'|'.join(fields)} VALUE")
        with self.eng.lock:
            s = self.state.settings
        if s is None:
            print("settings not read yet")
            return
        setattr(s, a[0], parse_bool(a[1]) if a[0] in ("wireless", "loopback") else int(a[1]))
        print(f"-> {midi.hex(host.settings_write(s))}")
        if self._confirm("write settings"):
            self.dev.write_settings(s)

    # ---- offline helpers ---------------------------------------------------------------------
    def do_decode(self, arg):
        """decode HEX... — parse a MIDI message and show what it means (doesn't touch the live state)"""
        data = parse_hex(arg.split())
        msg = midi.Msg.parse(data)
        fields = {k: getattr(msg, k) for k in ("kind", "ch", "cc", "val", "prog", "cmd", "dir", "what", "short_kind")
                  if getattr(msg, k) is not None}
        print(fields)
        if msg.payload:
            print(f"payload ({len(msg.payload)}): {midi.hex(msg.payload)}")
            if msg.kind == "nep" and len(msg.payload) > 8:
                print(f"unpacked:    {midi.hex(pack.unpack(msg.payload))}")
        ev = DeviceState().apply(data, Origin.PEDAL if msg.ch in (None, 0) else Origin.HOST)
        print(f"event: [{ev.category}] {ev}")

    def do_parselog(self, arg):
        """parselog FILE [N] — decode the first N (default 40) entries of a capture log offline"""
        a = shlex.split(arg)
        if not a:
            raise UsageError("parselog FILE [N]")
        n = int(a[1]) if len(a) > 1 else 40
        st = DeviceState()
        with open(a[0]) as f:
            entries = capture.parse(f.read())
        print(f"{len(entries)} entries")
        for e in entries[:n]:
            if e.kind == "mark":
                print(f"===== {e.text} =====")
            elif e.bytes is not None and e.origin is not None:
                ev = st.apply(e.bytes, e.origin)
                print(f"{'PEDAL' if e.origin == Origin.PEDAL else 'HOST '} [{ev.category:9}] {ev}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--listen", action="store_true", help="never send anything to the pedal")
    ap.add_argument("--port", help="connect to this MIDI input port instead of auto-connecting")
    ap.add_argument("--replay", help="play a capture log instead of a pedal")
    ap.add_argument("--speed", type=float, default=8.0, help="replay speed (default 8)")
    ap.add_argument("--timeout", type=float, default=1.0, help="SysEx reply timeout (s)")
    ap.add_argument("-q", "--quiet", action="store_true", help="start with the live monitor off")
    ap.add_argument("-c", dest="commands", action="append",
                    help="run this command after sync (repeatable), then exit")
    args = ap.parse_args()

    eng = Engine(args)
    eng.monitor = not (args.quiet or args.commands)
    sh = Shell(eng)
    try:
        if args.commands:
            if not eng.synced.wait(30):
                print("!! not synced after 30 s, running anyway")
            for c in args.commands:
                print(f"stomp> {c}")
                sh.onecmd(c)
            time.sleep(0.3)  # let sends go out / replies arrive
        else:
            while True:
                try:
                    sh.cmdloop()
                    break
                except KeyboardInterrupt:
                    print("^C")
                    sh.intro = ""
    finally:
        eng.close()


if __name__ == "__main__":
    main()
