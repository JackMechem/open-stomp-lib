"""Connect to the pedal (or replay a capture) and print everything that happens on it.

    python examples/monitor.py                         # auto-connect, sync, then follow the pedal
    python examples/monitor.py --listen                # send nothing at all
    python examples/monitor.py --port "JME STOMPBOX MIDI 1"
    python examples/monitor.py --replay ../../captures/logs/01-main-session.log --speed 8
    python examples/monitor.py --all                   # include protocol plumbing and repeats
"""

import argparse
import time

from open_stomp import Device, DeviceState, Origin, is_stomp_port
from open_stomp.records import preset_label


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--listen", action="store_true", help="never send anything to the pedal")
    ap.add_argument("--port", help="connect to this MIDI input port")
    ap.add_argument("--replay", help="play a capture log instead of a pedal")
    ap.add_argument("--speed", type=float, default=4.0, help="replay speed (default 4)")
    ap.add_argument("--all", action="store_true", help="show protocol plumbing and redundant values")
    args = ap.parse_args()

    state = DeviceState()
    synced = False
    with Device(sync=not args.listen, auto_connect=not (args.port or args.replay)) as dev:
        if args.port:
            dev.connect(args.port)
        if args.replay:
            dev.replay(args.replay, args.speed)
        for u in dev:  # blocks; Ctrl-C to stop
            if u.kind == "ports" and not any(is_stomp_port(p) for p in u.ports or []):
                print(f"waiting for the pedal… (MIDI inputs: {u.ports})")
            elif u.kind == "status" and u.status == "connected":
                print(f"connected to {u.port}" + ("" if u.output else " (input only)"))
            elif u.kind == "reset":
                synced = False
            elif u.kind == "sync" and u.total == 0 and not synced and state.firmware:
                synced = True
                cur = state.current
                name = state.preset_name(cur) if cur is not None else None
                label = preset_label(cur) if cur is not None else "?"
                print(f"synced: {state.product} firmware {state.firmware} · active {label} {name or ''}")
            elif u.kind == "mark":
                print(f"{clock(u.at)} ===== {u.text} =====")
            elif u.kind == "info":
                print(f"{clock(u.at)} {'! ' if u.error else ''}{u.text}")
                if u.text == "Replay finished":
                    break
            ev = state.feed(u)  # applies "message" updates, resets on "reset"
            if ev is not None:
                if (ev.category == "protocol" or ev.redundant) and not args.all:
                    continue
                who = "PEDAL" if u.origin == Origin.PEDAL else "HOST "
                print(f"{clock(u.at)} {who} {ev.label:8} {ev.subject:40} {ev.value}")


def clock(at: "float | None") -> str:
    t = at or 0.0
    return time.strftime("%H:%M:%S", time.localtime(t)) + f".{int(t * 1000) % 1000:03d}"


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
