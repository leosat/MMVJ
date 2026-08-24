# Steering Transformation

## General Overview

The **steering** transformation step emulates a physical steering wheel (or any  
rotational control surface) driven by a **relative** input source — most commonly  
a mouse X-axis movement. It converts incremental (relative) input into an  
**absolute wheel position** in the normalized range **\[-1, +1\]**, while layering  
on top of it:

*   **Force-feedback (FFB) displacement** — forces reported by the game through the  
    virtual HID device push the wheel position, simulating the torque a real wheel  
    would exert on the driver's hands.
*   **Autocentering** — an exponential-decay spring that pulls the wheel back toward  
    the center when the user is not actively turning, mimicking the self-aligning  
    torque of a real steering column.
*   **Hold factor** — a user-controllable "grip strength" parameter (0 = hands off  
    the wheel, 1 = locked grip) that scales how much FFB and autocentering are  
    allowed to move the wheel. This lets you simulate anything from a firm two-hand  
    grip to a loose one-hand hold, or even a completely free-spinning wheel.

### How it is supposed to be used

A typical sim-racing / flight-sim pipeline looks like this:

```
Mouse REL_X --> [ema / one_euro filter] --> [steering] --> Virtual Joystick ABS_X
                                                  ^
                                                  |  (reads FFB from the main pipleine destination
                                                  |   virtual joystick or an arbitrary data source)
```

1.  **Relative input** (mouse movement, trackball, etc.) enters the step.
2.  The step **accumulates** it into an absolute angle (by default accumulator is internal, but can be set to an external data source/destination).
3.  An optional **user-input sub-pipeline** (`integrated_user_input_transform`)  
    reshapes the accumulated curve (e.g. an exponential curve for a progressive  
    steering ratio).
4.  **Force feedback** read from the destination virtual device is filtered,  
    scaled, and added as a positional offset.
5.  **Autocentering** decays the position toward zero when idle.
6.  The final absolute value in **\[-1, +1\]** is written to the destination  
    (e.g. a virtual joystick axis).

The step runs on **every tick** — both on user-input events and on idle clock  
ticks (configurable via `global.idle_tick_rate`). Idle ticks are essential:  
they drive autocentering and FFB displacement even when the mouse is still.

> **Tip:** For the best experience, pair this step with a low-pass or one-euro  
> filter _before_ it (to smooth raw mouse jitter) and optionally another  
> filter _after_ it (to smooth the combined wheel output). Use the built-in  
> GUI telemetry (`--gui`) or `--debug-ff` to visualise the signal at every  
> stage.

### Wine / Proton note

When targeting games running under Wine, make sure the virtual joystick is  
overridden to **DInput** (not XInput) in the Wine control panel  
(`wine control joy.cpl`). XInput controllers do not expose the steering-wheel  
FFB effect types (Constant, Spring, Friction, Ramp) that this step relies on.

---

## Per-Parameter Reference

All parameters below are fields of the `steering:` mapping step in the YAML  
configuration. Parameters typed as **a static value or an arbitrary source** accept either a static number  
or a dynamic reference (`var: ...` / `dev: ... / ctl: ...`), allowing runtime  
tweaking from the GUI or from another mapping.

### `enabled` (bool, default: `true`)

Master on/off switch for the entire steering step. When `false`, the input  
value passes through unchanged.

---

### `input_gain` (a static value or an arbitrary source, in expected range of \[0, 1\], default: `0.33`)

_(YAML aliases:_ `_smoothing_alpha_`_,_ `_input_sensitivity_`_)_

Controls how much each unit of incoming relative movement rotates the wheel.  
The raw input value is multiplied by this factor **before** being accumulated  
into the wheel angle.

*   **Low values (0.05 - 0.15):** slow, heavy steering — large mouse sweeps  
    produce small wheel movements. Good for high-DPI mice or a "heavy truck"  
    feel.
*   **High values (0.3 - 0.5):** quick, responsive steering. Good for  
    low-DPI mice or arcade-style response.

Because this is a **static value or an arbitrary source**, you can bind it to a variable or a device  
control and adjust sensitivity on the fly (e.g. map it to a slider or a  
secondary mouse axis for a live "steering speed" knob).

---

### `auto_center_halflife` (a static value or an arbitrary source >= 0, default: `0.3`)

The **half-life** (in seconds) of the exponential decay that pulls the wheel  
toward center when the user is not providing input.

*   `0` — autocentering is **disabled**. The wheel stays wherever it was left.
*   `0.1` — very snappy return (~90% recentered in about 0.3 s).
*   `0.3` _(default)_ — moderate, natural-feeling return.
*   `1.0+` — slow, lazy drift back to center.

The per-tick decay formula is:

```
decay = (1 - 2^(-dt / halflife)) * (1 - hold_factor)
```

Autocentering only engages when **all** of the following hold:

1.  `halflife > 0`
2.  The user is **not** currently moving the input (`|delta| < epsilon`)
3.  Either FFB force is negligible **or** `auto_center_along_force_feedback > 0`

---

### `auto_center_along_force_feedback` (a static value or an arbitrary source, in expected range of \[0, 1\], default: `0.0`)

By default, autocentering is **suppressed** while a significant FFB force is  
present, because the game's force feedback is already providing the  
self-aligning torque. Setting this parameter to a positive value re-enables  
autocentering _alongside_ FFB, scaled by this factor:

*   `0.0` _(default)_ — autocentering only when FFB is approximately zero.
*   `0.5` — autocentering runs at half strength even during active FFB.
*   `1.0` — autocentering runs at full strength regardless of FFB.

Useful when the game's FFB is weak or absent and you still want the wheel  
to self-center.

---

### `hold_factor` (a static value or an arbitrary source, in expected range of \[0, 1\], default: `0.0`)

Simulates how firmly the driver grips the wheel. It scales **both** the FFB  
displacement and the autocentering decay by `(1 - hold_factor)`:

*   `0.0` _(default)_ — hands off the wheel. FFB and autocentering move the  
    wheel freely.
*   `0.5` — moderate grip. FFB and autocentering have half their usual effect.
*   `1.0` — locked grip. The wheel is completely immovable by FFB or  
    autocentering; only direct user input can turn it.

A common setup maps `hold_factor` to the mouse **Y-axis** (via an  
`integrate` + `s_curve` + `clamp` pipeline in a separate mapping), so that  
moving the mouse toward/away from you tightens or loosens your virtual grip.

---

### `accumulator` (DynValueRef, optional, default: none)

An optional reference to an external **variable** (or device control) that  
stores the raw accumulated wheel angle (`pre_filter` — the value _before_ the  
user-input sub-pipeline).

When set:

*   On each tick the step **loads** the current accumulator value as the  
    starting position.
*   After processing, it **writes back** the updated raw angle.

Use this when you need to share the raw wheel state across multiple mappings,  
or when you want a script to inspect or modify the unfiltered angle.

---

### `deadzone_counts` (float >= 0, default: `0.0`)

_(Reserved for future use — currently has no effect.)_

Intended to define a deadzone in input counts below which movement is ignored.

---

### `force_feedback` (object, optional)

Configures how force-feedback forces from the game are read, processed, and  
applied to the wheel position. When omitted or `enabled: false`, no FFB is  
applied and the wheel is driven purely by user input and autocentering.

#### `force_feedback.enabled` (bool, default: `true`)

Enable/disable FFB processing within this steering step.

#### `force_feedback.gain` (float >= 0, default: `1.0`)

Multiplies the (optionally filtered) FFB force before it is converted to a  
positional offset. Values > 1.0 amplify the effect; values \< 1.0 dampen it.  
Set to `0.0` to effectively disable FFB influence without removing the  
configuration block.

#### `force_feedback.invert` (bool, default: `false`)

Flips the sign of the FFB force. Use this if the wheel turns the wrong way  
in response to game forces (e.g. the wheel assists the turn instead of  
resisting it).

#### `force_feedback.component` (`X` | `Y`, default: `X`)

Selects which FFB component to read from the virtual device:

*   `X` _(default)_.
*   `Y`.

#### `force_feedback.transformation` (pipeline, default: `[]`)

An optional sub-pipeline applied to the raw FFB signal **before** gain and  
invert are applied. The pipeline receives the FFB value in **\[-1, +1\]**  
with relative (`Rel`) semantics.

Typical uses:

*   An `ema` or `one_euro` filter to smooth noisy FFB updates.
*   A `clamp` to limit peak forces.
*   A curve (`exp`, `signed_power`) to reshape the force response.

#### `force_feedback.custom_source` (a static value or an arbitrary source, optional)

Overrides the default FFB reading mechanism. Instead of reading the force from  
the destination virtual device's internal FFB state, the step reads from an  
arbitrary source (a variable, a device control, etc.). The value is  
mapped from the source's interval to **\[-1, +1\]**.

---

### `integrated_user_input_transform` (pipeline, default: `[]`)

A sub-pipeline applied to the **accumulated** (integrated) user input,  
**after** accumulation but **before** FFB and autocentering are added.

The pipeline receives the accumulated angle in **\[-1, +1\]** (absolute  
semantics) and must output a value in the same range.

Common configurations:

*   **Progressive steering ratio:** an `exp` curve with `base > 1` and  
    `center_symmetric: true` makes the center less sensitive and the extremes  
    more sensitive (like a real quick-ratio steering rack).
*   **Flat / linear response:** `base = 1.0` (or omit the pipeline entirely)  
    for 1:1 mapping.
*   **Inverted progressive:** `base < 1` (e.g. 0.975) makes the center _more_  
    sensitive and the extremes less so.

---

## Configuration Example

```
mappings:
  - name: Mouse Steering <> Wheel
    enabled: true
    source:
      dev: any_mouse
      ctl: REL_X
    destination:
      dev: Virtual steering wheel
      ctl: ABS_X
    transformation:
      # Smooth raw mouse input first
      - ema:
          enabled: true
          tau: 0.04
      # Main steering transform
      - steering:
          enabled: true
          input_gain: 0.1
          auto_center_halflife: 0.15
          auto_center_along_force_feedback: 0.0
          hold_factor:
            var: Steering Hold Factor
          force_feedback:
            enabled: true
            gain: 1.0
            invert: false
            component: X
            transformation:
              - ema:
                  enabled: true
                  tau: 0.01
          integrated_user_input_transform:
            exp:
              enabled: true
              base: 3.3
              center_symmetric: true
              
```