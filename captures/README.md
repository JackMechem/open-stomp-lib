# Capture logs (test vectors)

Traffic between the official Stomp Manager 1.0.2 and a Dual Stomp Lite on firmware V1.1.1, recorded by hooking
CoreMIDI inside the app ([docs/07](../docs/07-capture-methodology.md)). Copied from the
[open-stomp-manager](https://github.com/JackMechem/open-stomp-manager) repository, where they were made.

| File | Contents |
|---|---|
| `logs/00-selftest-identity.log` | Sniffer self-test: one identity ping |
| `logs/01-main-session.log` | About 2 h: connect handshake, every pedal control, every app action, rename/save, settings, NAM/IR uploads, each labelled with `# MARK` lines |

Format, one packet per line:

```
<unix ms> <TX|RX> <len> [<port>] <hex bytes…>     TX = app → pedal, RX = pedal → app
# MARK <unix ms> <text>                           what was about to be done
```

`open_stomp::capture::parse` reads them; `cargo test` uses them; `cargo run --example decode -- <log>` prints them.
