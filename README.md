# MMVJ -  Mouse and MIDI to Virtual Joystick (Transforming) Mapper for Linux.

[**Download binary pre-releases here**](https://github.com/leosat/MMVJ/releases/) or build manually with cargo if binary release [doens't work on your system](#troubleshooting) (takes a few minutes, [see below for instructions](#build-from-source)).  

NB: latest release includes steering indicator window, which is in draft as of now and is not disableable at compile-time. At runtie you can enale it with

```
--enable-steering-indicator-window true
```

or just close the window. Later I'll make this feature optional at compile-time.

---

## Features.

*   **Uses MIDI devices or Mouse/Mice and Trackpads as inputs.**
    *   Match mouse and MIDI devices by name and create separate mappings for them   
        (e.g. can attach many mice devices , optionally "hide" them from desktop usage with xinput   
        and use only for virtual joystick mappings).
*   **Creates virtual joysticks and uses them as output devices.**
    *   **Supports configurable joysticks persistence** across engine online hot-restarts when configuration changes.
*   **Supports config validation and hot-reload on configuration file changes.**
    *   If renewed config has errors, reports the error and continues running with previous configuration.
*   **Allows mappings of many inputs to many outputs.** 
*   **Provides out of the box advanced transformations**: **curves, filters, intuitive steering** **emulation and more**.
    *   Combine those discrete transformation steps arbitrarily to achieve desired effects.
    *   **A detail about steering transformation for use in simracing, flight and other simulator gaming:**
        *   **Supports force feedback**: accepts **constant force feedback** in application to **steering wheel movement emulation.**
        *   Supports configurable **autocentering**.
        *   Supports intuitive emulation o**f hands holding the steering wheel** with different force affecting the two mentioned above.
*   **Provides mouse and MIDI monitor and learn modes**: to automatically discover input devices and \[TODO\] generate relevant YAML configuration.
*   **Steering indicator window** displaying current joystick axis position for a joystick being affected by steering mapping transformation.
*   TODO: console steering indicator.
*   **TODO: Supports input devices hot-plugging**
    *   Reloads on devices plug/unplug events to match the renewed HW configuration without manual application restart requirement.

---

## Requirements.

*   Linux (uses evdev/uinput).
*   Rust >= 1.88.0.
*   ALSA for Rust crate midir for MIDI devices access (usually included by default in desktop Linux installations).
*   Membership in the `input` group or (generally not advised) root access.

## To test joysticks behaviours:

*   [My personal preference is jstest-gtk: https://github.com/Grumbel/jstest-gtk](https://github.com/Grumbel/jstest-gtk)
*   Or can use e.g. a command-line jstest utility , but it's not "visually" informative.

---

## Installation.

### Prerequisites: permissions.

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
git clone --depth 1 https://github.com/leosat/MMVJ

# Enter the repository clone directory
cd MMVJ

# Build the project
cargo build --release -j4

# Run the application
./target/release/mmvj --help
```

---

## Usage.

### Basic Usage.

```
# Run mapping engine with default configuration.
./target/release/mmvj

# Run mapping engine with custom configuration file.
./target/release/mmvj -c my_config.yaml

# Enable debug output.
./target/release/mmvj --debug
```

### Utility Commands.

```
# List available MIDI devices.
./target/release/mmvj enum-midi

# Monitor MIDI messages from a device.
./target/release/mmvj monitor-midi "Korg"

# Auto-learn MIDI controls.
./target/release/mmvj midi-learn

# List available mouse devices.
./target/release/mmvj enum-mice

# Monitor mouse events.
./target/release/mmvj monitor-mouse

# Validate configuration file.
./target/release/mmvj validate-config
```

---

## **Configuration**.

The application uses **YAML** configuration files to define:

*   **Inputs: MIDI Devices.**
*   **Inputs: Mouse Devices.**
*   **Outputs: Virtual Joysticks**: specifying properties and controls.
*   **Mappings**: multiple inputs can map to multiple outputs, each mapping having separate transformation pipeline.

### **Mapping Transformation Pipeline Steps.**

*   **Clamping:** can be used to saturate values at low/high bounds and optionally override current associated value range. 
*   **Inversion:** for both relative inputs or absolute values, within the defined range. 
*   **Integration**: linearly accumulates relative inputs within a specified range.
*   **Curves**: linear, quadratic, cubic, S-curve, smoothstep, exponential, etc.
*   **Steering**: emulating intuitive steering with...
    *   **Autocentering** with configurable dynamics via halflife-parametrized exponential decay. Very useful when no force feedback available.
    *   **Force feedback** (constant force supported) application to augment or be used instead of autocentering.
    *   **Steering Wheel "hands hold factor"** emulating how firmly your hands are holding the steering wheel.
        *   Affects autocentering and force feedback application dynamics.
    *   **Alpha-smoothing**.
*   **Filter for pedals emulation**: enabling smoother or intercorrelated pedal movements with 
    *   **Rize and fall rates** 
    *   **Fall rate hold factor**: other control state can be assigned to facilitate fall rate e.g. clutch fall rate can depend on throttle control value.
    *   **Fall timeout** can be used to facilitate value change without immediately going to "off" state (useful when discrete MIDI note events with distinct velocities are mapped to such a control). Further optional moving average filtering can facilitate this to simulate smoother value change.
*   **General filters:**
    *   **Moving average with configurable samples count.**
    *   **TODO: High-pass, low-pass, band-pass** with configurable steepness.
    *   **TODO: Convolution** with custom kernels.

---

## Configuration file reference:

*   [Configuration file stripped from comments to get a grasp of configuration itself](conf/mmvj_cfg.yaml).
*   For details please  [read the comments in the commented-out configuration file variant](conf/mmvj_cfg_WITH_COMMENTS.yaml) and 
*   See the [predefines config](conf/mmvj_cfg_predefines.yaml) for predefined controls that you can reference in your config.

## Example Configuration:

#### \[i\] Advanced example, default config:  [mmvj\_cfg.yaml.](conf/mmvj_cfg.yaml)

#### \[i\] Simplest example:

```
global:
  update_rate: 500
  persistent_joysticks: true

midi_devices:
  my_midi_keyboards:
    match_name_regex: ".*microKEY2.*"
    controls:
      my_pitch_wheel: PITCH_WHEEL

virtual_joysticks:
  gamepad:
    enabled: true
    persistent: true
    name: "Virtual Gamepad"
    properties:
      vendor_id: 0x123
      product_id: 0x456
      version: 0x789
    controls:
      axis_mundi: ABS_X

mappings:
  - source:
      device: my_midi_keyboards
      control: my_pitch_wheel
    destination:
      joystick: gamepad
      control: axis_mundi
    transformation:                               
      - s_curve:
          steepness: 8.0
```

## High-level architecture dependencies.

The application is built with:

*   **Tokio**: Async runtime for concurrent I/O.
*   **midir**: MIDI device access.
*   **evdev**: Linux input device access, virtual device creation.
*   **serde**: Configuration serialization.

## Performance

*   Low latency: \< 1ms processing time.
*   High update rate, configurable to 10000 Hz.

---

## Troubleshooting.

### Binary release run problems:

AppImage release.  

```
fuse: mount failed: Permission denied
Cannot mount AppImage, please check your FUSE setup.
You might still be able to extract the contents of this AppImage 
if you run it with the --appimage-extract option. 
See https://github.com/AppImage/AppImageKit/wiki/FUSE 
for more information
---------------------------------

Solution: run without trying to use fuse:


./mmvj --appimage-extract-and-run
```

### Raw binary release, problems with a dynamic library.

Libasound.so.2 not found (needed for MIDI devices input)

```
# Install a libasound2* library, 
# e.g. on Debian/Mint/Ubuntu/:

sudo apt-get install libasound2
```

Any other library:

```
Rebuild from source code, see instructions above.
```

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

# **WARNING/DISCLAIMER:**

This application is in active development state and is used as **a toy project** by the author **to learn the new programming language** (with all the consequences) and is provided as is without any warranties. **Not everything works and it is far from ideal currently**. Nevertheless, while still in development I'm finding it **already quite useful and capable**, so, I've decided to opensource it and provide for those who are looking for such a tool. When/if the project reaches production state, this warning will not be here.  

For any questions (or anything else) feel free to contact me at [leonid.satanovsky@gmail.com](mailto:leonid.satanovsky@gmail.com) (Leonid Satanovskiy).

## License.

All rights reserved. Copyright: Leonid Satanovskiy.  
When this app reaches production state this will be changed.  
/\* "GNU is not Unix." \*/ 

## Contributing.

Pull requests are **not yet accepted**,   
because please see the WARNING/DISCLAIMER at the top.  
It will change as soon as the project gets in production-ready state.