# One-Euro Filter Transformation

## General Overview

The **one\_euro** transformation step implements the
[1€ Filter](https://cristal.univ-lille.fr/~casiez/1euro/) — an adaptive
low-pass filter designed for noisy input signals such as mouse movement,
trackball data, or force-feedback readings. Unlike a fixed-cutoff EMA or
Butterworth filter, the 1€ Filter **dynamically adjusts its cutoff
frequency** based on the speed of the input signal:

* **Slow movements** → low cutoff → heavy smoothing → jitter and
  high-frequency noise are suppressed.
* **Fast movements** → high cutoff → light smoothing → the signal tracks
  the input with minimal lag.

This makes it uniquely suited for interactive control scenarios where you
need both **noise rejection at rest** and **low latency during rapid
motion** — exactly the trade-off encountered when emulating a steering
wheel from a mouse, smoothing force-feedback forces, or cleaning up any
analog sensor stream.


### How it is supposed to be used

A typical pipeline placement:

```
Mouse REL_X --> [one_euro] --> [steering] --> Virtual Joystick ABS_X
```

or as a final output smoother:

```
Source --> [steering] --> [one_euro] --> Virtual Joystick ABS_X
```

or inside a force-feedback sub-pipeline:

```
FFB raw --> [one_euro] --> gain/invert --> steering FFB offset
```

1. The raw (possibly noisy) input value enters the step.
2. The filter computes the adaptive cutoff from the signal's rate of
   change and smooths accordingly.
3. The filtered value is passed to the next transform step or to the
   mapping destination.

The step runs on **every tick** — both on user-input events and on idle
clock ticks (configurable via `global.idle_tick_rate`). For **relative**
inputs, idle-tick behaviour is configurable:

* By default, relative inputs are **not** fed on idle ticks (the filter
  is skipped).
* `on_relative_input_feed_on_idle: true` — the last known value is
  re-fed on idle ticks (keeps the filter converging).
* `on_relative_input_reset_on_idle: true` — the filter state is reset
  to the current input on idle ticks (useful when the relative source
  goes silent and you want the filter to snap to the new baseline).

> **Tip:** The 1€ Filter is a drop-in replacement for `ema` whenever you
> need **speed-adaptive** smoothing. If a fixed time constant is
> sufficient, `ema` is simpler and marginally cheaper. Use `one_euro`
> when the signal alternates between slow precision movements and fast
> sweeps.

> **Tip:** Use the built-in GUI telemetry (`--gui`) or `--debug` modes
> to visualise the raw vs. filtered signal. The per-step **live monitor
> graph** (blue = input, red = output) is invaluable for tuning
> `min_cutoff_hz` and `beta`.

---

## Per-Parameter Reference

All parameters below are fields of the `one_euro:` mapping step in the
YAML configuration.

### `enabled` (bool, default: `true`)

Master on/off switch for the entire one-euro step. When `false`, the
input value passes through unchanged and no filter state is updated.

---

### `min_cutoff_hz` (float > 0, default: `1.0`)

The **minimum cutoff frequency** (in Hz) of the adaptive low-pass
filter. This is the cutoff used when the input signal is **stationary
or moving very slowly**.

* **Low values (0.1 – 0.5):** very heavy smoothing at rest. Excellent
  jitter suppression but introduces noticeable lag when motion begins.
* **Default (1.0):** moderate smoothing — a good starting point for
  most mouse / trackball inputs.
* **High values (5.0 – 50.0):** light smoothing even at rest. The
  filter behaves almost like a passthrough for slow movements. Use
  when the input is already clean or when minimal latency is critical.

Think of `min_cutoff_hz` as the **noise floor** of the filter: it
determines how aggressively high-frequency jitter is removed (when the
user is holding still or... is "jittering" oneself :) ).

---

### `beta` (float ≥ 0, default: `0.007`)

The **speed coefficient** that controls how much the cutoff frequency
increases in response to fast input movement.

* **`0.0`:** the filter degenerates into a **fixed-cutoff** low-pass
  with cutoff = `min_cutoff_hz`. Speed has no effect.
* **Small values (0.001 – 0.01):** gentle adaptation. Fast movements
  receive slightly less smoothing. Good default range for most use
  cases.
* **Large values (0.1 – 10.0):** aggressive adaptation. The cutoff
  ramps up quickly during fast sweeps, yielding near-unfiltered
  passthrough for rapid motion. May re-introduce jitter during fast
  movements if set too high.

The adaptive cutoff formula is:

```
cutoff = min_cutoff_hz + β · |dx̂|
```

where `dx̂` is the smoothed derivative (speed) of the signal. A higher
`β` makes the filter more "transparent" during fast motion at the
expense of potentially letting noise through.

> **Tuning heuristic:** Start with `beta: 0.0` and increase until fast
> movements feel responsive enough, then back off slightly.

---

### `d_cutoff_hz` (float > 0, default: `1.0`)

The **cutoff frequency** (in Hz) of the low-pass filter applied to the
**derivative** (speed estimate) of the signal, before it is used to
compute the adaptive cutoff.

The raw derivative `(x - x̂_prev) / dt` is extremely noisy (it
amplifies sensor jitter by `1/dt`). This secondary filter smooths the
derivative so that the adaptive cutoff does not oscillate wildly.

* **Low values (0.01 – 0.1):** very smooth derivative estimate. The
  adaptive cutoff reacts slowly to speed changes — more stable but
  less responsive.
* **Default (1.0):** balanced derivative smoothing.
* **High values (10.0 – 100.0):** the derivative is barely filtered.
  The adaptive cutoff tracks speed changes almost instantly but may
  oscillate if the input is noisy.

In practice, `d_cutoff_hz` rarely needs adjustment. The default of
`1.0` works well for most input devices.

---

### `on_relative_input_feed_on_idle` (bool, default: `false`)

Controls whether the filter **continues processing** on idle ticks when
the input has **relative** semantics (e.g. mouse `REL_X`).

* `false` *(default)* — on idle ticks the filter is **skipped**; the
  last filtered output is held. This is usually desired: when the
  mouse stops moving, there is no new information to filter.
* `true` — the last known input value is **re-fed** into the filter on
  every idle tick. The filter output continues to converge toward the
  last sample. Useful when you want the output to "settle" smoothly
  after the input stops.

> **Warning:** behaviour depends on the idle tick frequency
> (`global.idle_tick_rate`). At very high tick rates, re-feeding
> converges almost instantly; at low rates, the settling is more
> gradual.

This parameter is **ignored** for absolute inputs (they are always
processed on every tick).

---

### `on_relative_input_reset_on_idle` (bool, default: `false`)

Controls whether the filter's **internal state is reset** on idle ticks
when the input has relative semantics.

* `false` *(default)* — state is preserved across idle ticks.
* `true` — on each idle tick the filter state (`x̂_prev`, `dx̂_prev`)
  is **reset** to the current input value. The next real input event
  is treated as if it were the first sample (no smoothing history).

Mutually exclusive in effect with `on_relative_input_feed_on_idle`:
enabling one disables the other in the GUI.

Use this when the relative source may jump to a new baseline after a
pause (e.g. a trackball that is lifted and repositioned) and you want
to avoid a smoothing transient.

---

## Configuration Example

### Smoothing mouse input before a steering transform

```yaml
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
      # Adaptive smoothing of raw mouse jitter
      - one_euro:
          enabled: true
          min_cutoff_hz: 1.0
          beta: 0.007
          d_cutoff_hz: 1.0
      # Steering wheel emulation
      - steering:
          enabled: true
          input_gain: 0.1
          auto_center_halflife: 0.15
```

### Final output smoother after steering

```yaml
    transformation:
      - steering:
          enabled: true
          input_gain: 0.12
      # Gentle final pass to eliminate residual high-frequency wheel oscillation
      - one_euro:
          enabled: true
          min_cutoff_hz: 3.0
          beta: 0.004
          d_cutoff_hz: 1.0
```

### Inside a force-feedback sub-pipeline

```yaml
      - steering:
          enabled: true
          input_gain: 0.1
          force_feedback:
            enabled: true
            gain: 1.0
            transformation:
              - one_euro:
                  enabled: true
                  min_cutoff_hz: 2.0
                  beta: 0.01
                  d_cutoff_hz: 0.5
```

### Algorithm (per tick)

Given a new raw sample `x` at time `t`:

1. **Derivative estimation**

   ```
   dx = (x - x̂_prev) / dt
   ```

2. **Derivative smoothing** — a low-pass filter with a fixed cutoff
   `d_cutoff_hz` is applied to the derivative to suppress noise in the
   speed estimate:

   ```
   dx̂ = LPF(dx, α_d)       where α_d = smoothing_factor(dt, d_cutoff_hz)
   ```

3. **Adaptive cutoff** — the cutoff for the main signal filter is
   computed from the smoothed speed:

   ```
   cutoff = min_cutoff_hz + β · |dx̂|
   ```

4. **Signal smoothing** — a low-pass filter with the adaptive cutoff is
   applied to the raw sample:

   ```
   x̂ = x̂_prev + α · (x - x̂_prev)    where α = smoothing_factor(dt, cutoff)
   ```

5. The smoothing factor helper is:

   ```
   smoothing_factor(dt, f_c) = 1 / (1 + τ / dt)
   τ = 1 / (2π · f_c)
   ```

The filter maintains internal state (`x̂_prev`, `dx̂_prev`, `t_prev`)
across ticks. State can be **reset** explicitly (e.g. on idle ticks for
relative inputs — see `on_relative_input_reset_on_idle`).
