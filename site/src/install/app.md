# Install the app

Download a package for your system from the
[releases page](https://github.com/JackMechem/open-stomp-manager/releases):

| Platform | Package | Contents |
|---|---|---|
| Linux (x86_64) | `…-linux-x86_64.tar.gz` | the binary, a `.desktop` file and an icon |
| macOS 11+ (Apple Silicon and Intel) | `…-macos-universal.zip` | `Open Stomp Manager.app` |
| Windows 10+ (x86_64) | `…-windows-x86_64.zip` | `open-stomp-manager.exe` |

Then plug the pedal in over USB and start the app, in either order. It finds the pedal by itself and reads all 99
presets, the live sound and the settings. No driver is needed on any system: the pedal is a class-compliant USB
audio and MIDI device.

## Per platform

**Linux.** Unpack the archive and run `open-stomp-manager`. To get it in your application menu, copy the
`.desktop` file to `~/.local/share/applications/` and the icon to `~/.local/share/icons/`. The app needs ALSA
(`libasound`), which every desktop distribution has.

**macOS.** The app is ad-hoc signed, not notarized. The first time, right-click it and choose *Open*, or run:

```sh
xattr -dr com.apple.quarantine "Open Stomp Manager.app"
```

**Windows.** A MIDI port can be open in only one program at a time, so quit the official Stomp Manager first.

**Nix.**

```sh
nix run github:JackMechem/open-stomp-manager            # the latest commit
nix run github:JackMechem/open-stomp-manager/v0.7.0     # a release
```

On NixOS, add it to a flake-based system:

```nix
# flake.nix
inputs.open-stomp-manager.url = "github:JackMechem/open-stomp-manager/v0.7.0";

# configuration.nix
environment.systemPackages = [ inputs.open-stomp-manager.packages.${pkgs.system}.default ];
```

## Try it without a pedal

The app can replay a recorded session between the official app and the pedal:

```sh
open-stomp-manager --replay 01-main-session.log --speed 8
```

The capture logs are in the app repository's
[`captures/logs`](https://github.com/JackMechem/open-stomp-manager/tree/main/captures/logs).

## Next

* The [user guide](../manager/user-guide.md) covers everything in the window, including
  [importing NAM models](../manager/user-guide.md#importing-a-nam-model) and
  [IRs](../manager/user-guide.md#importing-an-ir).
* To build it yourself, see [building](../manager/building.md).
