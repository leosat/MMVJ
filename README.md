# MMVJ - Mouse and MIDI to Virtual Joysticks (Transforming) Mapper for Linux.

**WARNING/DISCLAIMER:**

This application is still in early development state, is used as **a toy project** by the author **to learn the new programming language** (with all the consequences) and enjoy Rirchard Burns Rally RSF mod on his Linux machine. And is provided as is without any warranties. Have fun at your own risk! :). When/if the project reaches production state, this warning will not be here.  

For any questions feel free to contact me at [leonid.satanovsky@gmail.com](mailto:leonid.satanovsky@gmail.com) (Leonid Satanovskiy).

---

## Features.

*   **MIDI Device Support**: Map any MIDI controller to virtual joystick inputs.
*   **Mouse/Trackpad Support**: Use mouse movements as joystick inputs including intuitive emulation for steering wheel movement, see below.
*   **Virtual Joystick Creation**: Create multiple virtual joysticks with custom configurations.
*   **Advanced Transformations**: Apply curves, filters, emulate intuitive steering and more. Combine those discrete transformation steps arbitrarily to achieve desired effects (see below).
*   **Force Feedback**: Supporting constant force feedback application to steering transformation.
*   **MIDI Learn Mode**: Automatically discover and configure MIDI controls.
*   **TODO: Hot-plugging**: Automatic device detection and reconnection.
*   **TODO: Config hot-reload**: Automatic hadnling of configuration file changes.

---

## Requirements.

*   Linux (uses evdev/uinput).
*   Rust 1.70 or newer.
*   Membership in the `input` group or (generally not advised) root access.

---

## Installation.

### Prerequisites.

```
# Add user to input group (for virtual joystick creation)
sudo usermod -a -G input $USER
# Logout and login again for group changes to take effect

# Enable uinput module (for force feedback)
sudo modprobe uinput
```

### Build from Source.

```
# Clone the repository
cd mmvj-rust

# Build the project
cargo build --release

# Run the application
./target/release/mmvj --help
```

---

## Usage.

### Basic Usage.

```
# Run with default configuration
./mmvj

# Run with custom configuration
./mmvj --config my_config.yaml

# Enable debug output
./mmvj --debug
```

### Utility Commands.

```
# List available MIDI devices
./mmvj enum-midi

# Monitor MIDI messages from a device
./mmvj monitor-midi "Korg"

# Auto-learn MIDI controls
./mmvj midi-learn

# List available mouse devices  
./mmvj enum-mice

# Monitor mouse events
./mmvj monitor-mouse

# Validate configuration file
./mmvj validate-config
```

---

## Configuration.

The application uses **YAML** configuration files to define:

*   **Input: MIDI Devices**: by pattern-matching for MIDI controllers names and controls.
*   **Input: Mouse Devices**: by pattern matching for mice/trackpads.
*   **Output: Virtual Joysticks**: by specifying properties and controls.
*   **Mappings**: multiple inputs can map to multiple outputs, each mapping having separate transformation pipelines.
    *   **Transformations**: a configured sequence of discrete transformation steps chaging values from input to output.

### **Transformation Pipeline(s) Steps.**

**Mappings** support changing values from input to output   
by **combining** powerful discrete transformation steps  
**in any meaningful desired sequence**.

The steps available are:

*   **Clamping:** can be used to saturate values at low/high bounds and optionally override current associated value range. 
*   **Inversion:** for both relative inputs or abolute values, within the defined range. 
*   **Integration**: linearly accumulates relative inputs within a specified range.
*   **Curves**: linear, quadratic, cubic, S-curve, smoothstep, exponential, etc.
*   **Steering**: emulating intuitive steering with...
    *   **Autocentering** with configurable dynamics via halflife-parametrized exponential decay. Very useful when no force feedback available.
    *   **Force feedback** (constant force supported) application to augment or be used instead of autocentering.
    *   **Steering Wheel "hands hold factor"** emulating how firmly your hands are holding the steering wheel.
        *   Affects autocentering and force feedback application dynamics.
    *   **Alpha-smoothing**.
*   **Pedal-specific filters**: emulate smoother pedal movements with 
    *   **Rize and fall rates** (can be also configured to be dynamically influenced ty other controls, e.g. clutch fall rate can depend on throttle control value).
*   **TODO: General filters:**
    *   **High-pass, low-pass, band-pass** with configurable steepness.
    *   **Convolution** with custom kernels.

---

## Example Configuration:

### \[i\] Advanced example:  [mmvj\_cfg.yaml](mmvj_cfg.yaml)

### \[i\] The simplest example:

```
global:
  update_rate: 1000
midi_devices:
  my_keyboard:
    match_criteria:
      name_regex: ".*Korg.*"
    controls:
      pitch_wheel:
        predefined_type: pitch_wheel
virtual_joysticks:
  gamepad:
    properties:
      name: "Virtual Gamepad"
    controls:
      axis_x:
        type: axis
        code: ABS_X
        range: [-32768, 32767]
mappings:
  - source:
      device: my_keyboard
      control: pitch_wheel
    destination:
      joystick: gamepad
      control: axis_x
    transformation:
      curve:
        type: s_curve
        parameters:
          steepness: 8.0
```

## High-level architecture dependencies.

The application is built with:

*   **Tokio**: Async runtime for concurrent I/O.
*   **midir**: MIDI device access.
*   **evdev**: Linux input device access, virtual device creation.
*   **serde**: Configuration serialization.

## Performance

*   Low latency: \< 1ms processing time
*   High update rate, configurable to 1000 Hz

---

## Troubleshooting.

Permission denied errors.

```
# Option 1: Add to input group (recommended)
sudo usermod -a -G input $USER
# Logout and login

# Option 2: Run as root (not recommended)
sudo ./mmvj
```

Force Feedback isn't working.

```
# Load uinput module
sudo modprobe uinput

# Check if module is loaded
lsmod | grep uinput

# Make uinput persistent
echo "uinput" | sudo tee -a /etc/modules
```

MIDI Device not found.

```
# List all MIDI devices
./mmvj enum-midi

# Check ALSA MIDI devices
aconnect -l

# Check permissions
ls -la /dev/snd/
```

## License.

All rights reserved. Copyright: Leonid Satanovskiy.  
When this app reaches production state this will be changed.  
"GNU is not Unix", as we all know.  

## Contributing.

Pull requests are **not yet** welcomed, because please see the WARNING/DISCLAIMER at the to.  
It will change as soon as the project gets in production-ready state.