# Contributing to open-stomp

Thanks for your interest in open-stomp! You don't need to know Rust or own the pedal to help: bug reports,
captures, docs and testing on other hardware are all valuable.

## Ways to help

- **Report a bug or ask a question.** [Open an issue](https://github.com/JackMechem/open-stomp-lib/issues/new/choose).
- **Test other hardware.** If you own a Dual Stomp, Tiny Stomp, or a pedal on different firmware, tell us what
  works and what doesn't.
- **Answer an open question.** [docs/09](docs/09-open-questions.md) lists what's still unknown about the
  protocol, and how each question could be answered.
- **Improve the docs.** Fixes to unclear or wrong documentation are always welcome.
- **Write code.** See [where help is wanted](#where-help-is-wanted) below.

For anything bigger than a small fix, please open an issue first so we can agree on the approach before you spend
time on it.

## Which repository?

The project has two repositories:

| Repository | What goes there |
|---|---|
| **open-stomp-lib** (this one) | The protocol, device behaviour, NAM/IR conversion, Python bindings, protocol docs |
| [open-stomp-manager](https://github.com/JackMechem/open-stomp-manager) | The desktop app's UI and packaging, and the capture and reverse-engineering tools |

A new app feature that needs the pedal to do something new usually lands here first, then in the app.

## Setting up

You need [Rust](https://rustup.rs) 1.88 or newer. On Linux, also install the ALSA and D-Bus headers:

```sh
sudo apt install libasound2-dev libdbus-1-dev pkg-config     # Debian, Ubuntu
sudo dnf install alsa-lib-devel dbus-devel pkgconf           # Fedora
```

macOS and Windows need nothing extra. With Nix, `nix develop` gives you a shell with everything.

```sh
git clone https://github.com/JackMechem/open-stomp-lib
cd open-stomp-lib
cargo test
```

### Project layout

```
src/              the library; docs/10 has the module map
bindings/python/  Python bindings (PyO3 + maturin): Rust glue, typed stubs, tests, examples
examples/         command-line tools: monitor, decode, ports, and NAM/IR conversion checks
tests/            integration tests driven by capture logs, and NAM inference vs NeuralAmpModelerCore
captures/logs/    recordings of the official app and the pedal, used as test vectors
docs/             protocol documentation and the library guides
site/             the documentation site (mdBook)
tools/pedalsim/   Python research behind the optimised NAM/IR conversions (pedal simulator, re-amp rig)
scripts/          release helpers used by CI
```

### Working without a pedal

`captures/logs/` holds recordings of the official app talking to the pedal. The tests use them as test vectors, and
you can replay them through the real connection engine:

```sh
cargo run --example monitor -- --replay captures/logs/01-main-session.log --speed 8
cargo run --example decode -- captures/logs/01-main-session.log     # print a log as readable events
```

### Working with a pedal

Live edits only change the pedal's current sound, so they are safe to experiment with. Preset and settings
**writes** overwrite stored data: back up your presets first. `cargo run --example monitor -- --listen` watches the
pedal without sending anything.

## Checks

CI runs these on Linux, macOS and Windows. Run them before opening a pull request:

```sh
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo test
cargo test --no-default-features
```

If you changed the Python bindings:

```sh
cd bindings/python
python -m venv .venv && . .venv/bin/activate
pip install maturin pytest mypy
maturin develop
pytest
python -m mypy.stubtest open_stomp --ignore-missing-stub    # the .pyi stubs match the compiled module
```

## Code guidelines

- **Match the surrounding code.** The codebase isn't formatted with `rustfmt`, so please don't reformat code you
  aren't otherwise changing. Lines go up to about 120 columns.
- **Keep the protocol layer pure.** Everything outside `device.rs`, `ble.rs` and `nam/` must build with
  `--no-default-features`: no I/O and no dependencies.
- **Test against captures.** Protocol changes should come with a test, ideally against a real capture in
  `captures/logs/` or a fake pedal in `tests/offline_pedal.rs`.
- **Keep both APIs in step.** A new public Rust API usually needs a Python binding in `bindings/python/src` and a
  matching stub in `bindings/python/python/open_stomp/*.pyi`.
- **Document what you learn.** New protocol findings go in the matching `docs/` page.

## Pull requests

1. Fork the repository and create a branch from `main`.
2. Make your change, with tests and docs where they apply.
3. Add a line to the `[Unreleased]` section of [CHANGELOG.md](CHANGELOG.md) if users would notice the change.
4. Run the [checks](#checks).
5. Open a pull request that says what changed and why. Link the issue if there is one, and say whether you tested
   on a real pedal (model and firmware).

Keep pull requests focused: one change per pull request is easier to review than several mixed together.

## Where help is wanted

- **Run preset and settings writes on hardware.** They match the official app byte for byte, but haven't been run
  on a pedal from this code yet.
- **LSTM and ConvNet NAM models.** Only WaveNet models convert today.
- **A command-line tool** (`stompctl`): device info, monitoring, and preset get/set/backup.
- **Preset backup and restore:** a file format and library API for it (`bindings/python/examples/shell.py` has a
  basic `backup` command).
- **Open protocol questions:** loop mode (CC `0x4A`), tuner control, unknown preset and settings bytes, and
  whether the edit buffer can be written. See [docs/09](docs/09-open-questions.md).

## Documentation

The docs are Markdown in `docs/`. The [documentation site](https://jackmechem.github.io/open-stomp-lib/) is built
from them, together with the app's docs, by `site/` (mdBook). To preview it, with an open-stomp-manager checkout
next to this one:

```sh
python3 site/assemble.py && mdbook serve site
```

## Releases

Releases are made by the maintainer. The version lives in `Cargo.toml` (`[workspace.package]`) and follows
[SemVer](https://semver.org).

1. Bump the version in `Cargo.toml`, add a `## [x.y.z]` section to `CHANGELOG.md`, update the version and the
   `tag = "vx.y.z"` in the README, and run `cargo build` so `Cargo.lock` follows. `scripts/check-version.sh` checks
   all of this.
2. Commit, then `git tag vx.y.z && git push --tags`. The release workflow tests the tag, builds the Python wheels
   and publishes a GitHub release with the changelog section as its notes.
3. Point Open Stomp Manager at the new tag: `OPEN_STOMP_LIB_REF` in its workflows, and
   `nix flake update open-stomp-lib` there once the `stable` branch has moved.

A pre-release is a `vx.y.z-beta.n` tag with that version in `Cargo.toml`. It needs no README or changelog changes.
After each release, `.github/workflows/channels.yml` moves the `stable` branch to the newest release and
`unstable` to the newest release or pre-release.

## License

By contributing, you agree that your contributions are dual-licensed under [MIT](LICENSE-MIT) and
[Apache-2.0](LICENSE-APACHE), like the rest of the project.
