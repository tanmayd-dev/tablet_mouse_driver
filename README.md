# tablet-driver

Translate a Wacom tablet into a **virtual mouse** or **virtual Xbox 360
gamepad** on Linux. It was built for Minecraft (PineconeMC), Trackmania on
Steam, and *The Uncanny Return* (`../small_bad`), but works with any game that
reads standard mouse or gamepad input.

The driver is deliberately narrow:

- Mouse presets emit **only mouse events**. WASD, jump, inventory, and every
  other key binding stay in the game's own settings.
- Gamepad presets emit **only gamepad events**. Tablet throttle/brake is off by
  default, so keyboard pedals work untouched.
- There is **no keyboard output** at all.

## Requirements

- Linux with `evdev` and `uinput`.
- Read access to `/dev/input/event*` and write access to `/dev/uinput`
  (membership in the `input` group is usually enough).
- Rust 1.75+ to build.

Check permissions:

```sh
id -nG | tr ' ' '\n' | grep -x input
ls -l /dev/uinput
```

## Build and install

```sh
cargo build --release
install -Dm755 target/release/tablet-driver ~/.local/bin/tablet-driver
```

Install zsh completion (writes to `~/.local/share/zsh/site-functions`):

```sh
tablet-driver completion zsh --install
```

That directory is already on `fpath` in the default setup used here. Start a
new shell or run `compinit` afterwards.

## Quick start

```sh
tablet-driver probe                    # list detected input devices
tablet-driver --preset minecraft       # relative mouse look + clicks
tablet-driver --preset trackmania      # analog steering, keyboard pedals
tablet-driver --preset small-bad       # mouse aiming for The Uncanny Return
tablet-driver --preset desktop         # trackpad-style cursor + scrolling
```

`tablet-driver run --preset ...` is equivalent; the `run` subcommand is
inserted automatically when the first argument is an option.

Stop with `Ctrl-C`. The driver releases every button, centres steering, and
ungrabs the tablet before exiting.

## Presets

| Preset | Output | Grab pen | Buttons |
| --- | --- | --- | --- |
| `minecraft` | relative mouse | yes | tip LMB, lower RMB, upper MMB, ExpressKeys 0/1 wheel |
| `trackmania` | Xbox 360 gamepad | no | all unbound |
| `small-bad` | relative mouse | yes | tip LMB, lower RMB, upper MMB, ExpressKeys 0/1 wheel |
| `desktop` | relative mouse | yes | tip LMB, lower RMB, upper MMB, ExpressKeys 0/1 wheel |

List presets and the directories searched:

```sh
tablet-driver presets
tablet-driver presets --plain   # names only, used by zsh completion
```

### User presets

Put a TOML file in `~/.config/tablet-driver/presets/<name>.toml`. User presets
shadow built-ins with the same name and are completed automatically:

```sh
tablet-driver --preset my-shooter
```

## Configuration

A config file is overlaid on a preset. It may select its own base preset:

```toml
preset = "trackmania"

[gamepad]
enable_pedals = true
steering_curve = 1.0

[buttons]
pad_0 = "btn_west"   # respawn
pad_3 = "btn_start"  # pause
```

```sh
tablet-driver --config ./trackmania-pedals.toml
tablet-driver config --config ./trackmania-pedals.toml   # print resolved TOML
```

Command-line flags are applied last and win over the file:

```sh
tablet-driver --preset trackmania --no-pedals --steering-curve 1.5
```

### Schema

```toml
[general]
mode = "mouse"          # "mouse" | "gamepad"
grab_pen = true         # exclusive EVIOCGRAB on the pen node
grab_pad = false
lift_centers = true     # centre steering when the pen leaves the tablet
# device = "/dev/input/event15"
# pad_device = "/dev/input/event16"

[mouse]
scheme = "delta"        # "delta" (swipe) | "rate" (joystick deflection)
sensitivity = 1.0
scale = 0.04            # raw tablet counts to mouse counts
invert_x = false
invert_y = false
deadzone = 0.05         # rate mode only
rate_speed = 1200.0     # rate mode only, counts per second

[gamepad]
steering_axis = "x"     # "x" | "y"
steering_center = 0.5
steering_half_span = 0.5
steering_deadzone = 0.04
steering_curve = 1.3    # 1.0 = linear, >1.0 = finer centre
invert_steering = false
enable_pedals = false   # off by default so keyboard pedals work
pedal_scheme = "split"  # "split" | "pressure" | "combined"
pedal_deadzone = 0.03
pedal_curve = 1.0

[buttons]
pen_tip = "btn_left"
pen_barrel_lower = "btn_right"
pen_barrel_upper = "btn_middle"
pad_0 = "wheel_up"
pad_1 = "wheel_down"
pad_2 = "none"
pad_3 = "none"
```

### Actions

| Kind | Names |
| --- | --- |
| none | `none` |
| mouse buttons | `btn_left`, `btn_right`, `btn_middle`, `btn_side`, `btn_extra` |
| wheel | `wheel_up`, `wheel_down` |
| face buttons | `btn_a`/`btn_south`, `btn_b`/`btn_east`, `btn_x`/`btn_north`, `btn_y`/`btn_west` |
| system | `btn_start`, `btn_select`, `btn_mode` |
| sticks | `btn_thumbl`, `btn_thumbr` |
| bumpers | `btn_tl`/`btn_shoulder_left`, `btn_tr`/`btn_shoulder_right` |
| d-pad | `btn_dpad_up`, `btn_dpad_down`, `btn_dpad_left`, `btn_dpad_right` |
| steering | `recenter` |

Aliases such as `left`, `a`, `lb`, `l3`, and `dpad_up` are accepted. Keyboard
names are rejected on purpose.

## Trackmania pedals

Tablet throttle and brake are **disabled by default**. Use the keyboard for
throttle/brake and the pen only for steering. To let the tablet drive pedals:

```sh
tablet-driver --preset trackmania --pedals
```

Choose how in the config:

- `split` — upper half of the tablet is throttle, lower half is brake.
- `pressure` — stylus pressure is throttle.
- `combined` — vertical position drives `ABS_Y`.

Force them back off with `--no-pedals`; this always wins over the config.

## Device selection

By default the driver scans `/sys/class/input`, picks the highest-scoring pen,
and pairs it with a companion ExpressKey pad that shares the same vendor/product
ID. Override either path:

```sh
tablet-driver --preset minecraft --device /dev/input/event15
tablet-driver --preset minecraft --pad-device /dev/input/event16
```

`tablet-driver probe` prints every device with its axis ranges and pressure
range, plus a `pen`/`pad`/`other` label.

## Zsh completion

```sh
tablet-driver completion zsh            # print to stdout
tablet-driver completion zsh --install  # install into the user data dir
```

The completion is dynamic: `--preset <TAB>` lists built-in and user presets by
calling `tablet-driver presets --plain`. Bash and fish completion are also
available through clap's generator:

```sh
tablet-driver completion bash --install
tablet-driver completion fish --install
```

## Troubleshooting

**Permission denied opening the tablet or `/dev/uinput`.**
Add your user to the `input` group and log out/in:

```sh
sudo usermod -aG input "$USER"
```

**The camera spins or the cursor jumps when the pen is lifted.**
Keep `grab_pen = true` for mouse presets. The driver already discards the
kernel's `(0, 0)` coordinate packet that arrives with `BTN_TOOL_PEN=0`.

**Trackmania does not see a controller.**
The virtual pad uses the Microsoft Xbox 360 IDs (`045e:028e`) so Proton and SDL
recognise it. Check that it appears while the driver runs:

```sh
ls /dev/input/by-id
```

If a physical controller is already connected, disconnect it or use Steam's
controller settings to pick the virtual pad.

**Two tablets / a virtual device is picked instead of the Wacom.**
Run `tablet-driver probe` and pass `--device` explicitly.

**Wayland / dwl.**
The driver works at the evdev/uinput level, below the compositor. Grabbing the
pen prevents absolute coordinates from reaching the compositor while a game
holds relative pointer lock.

## Development

```sh
cargo test
cargo run -- --preset minecraft --dry-run --no-grab
cargo run -- probe
```

`--dry-run` logs actions without creating `/dev/uinput` devices. All mapping
math and engine behaviour is unit-tested with mock sinks, so the test suite
does not need a tablet or uinput access.
