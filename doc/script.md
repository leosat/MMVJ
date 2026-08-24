# Script Transformation

## General Overview

The **script** transformation step embeds a user-supplied **Luau** script that
runs on **every tick** of the mapping pipeline. It gives you full
programmatic control over the signal: you can read the main pipeline input,
fetch values from arbitrary auxiliary data sources, run computations in a
real programming language (loops, conditionals, math, persistent state), and
write results back to the main pipeline output and/or any number of auxiliary
destinations.

Key capabilities:

* **Luau scripting** — a fast, sandboxed scripting language (a dialect of
  Lua 5.1 with gradual typing) compiled once and executed every tick via
  `mlua`. Global variables **persist** between ticks, so the script can
  maintain its own state (counters, integrators, flags, etc.) without any
  external storage.
* **Auxiliary sources (`aux_srcs`)** — read values from any number of
  external data sources (variables, device controls, static values)
  alongside the main pipeline input. Each source can be optionally
  **remapped** to a custom interval before the script sees it.
* **Auxiliary destinations (`aux_dsts`)** — write computed values to any
  number of external targets (variables, device controls). Each destination
  can be optionally **remapped** from a custom interval to the target's
  native range.
* **Named sub-pipelines (`aux_transformations`)** — define reusable
  transformation pipelines (e.g. an `ema` filter, an `exp` curve) in YAML
  and invoke them from the script by name via the `transform()` API
  function. This lets you combine the declarative pipeline system with
  imperative scripting.

### How it is supposed to be used

A typical scripting pipeline looks like this:

```
Source --> [optional pre-filters] --> [script] --> [optional post-filters] --> Destination
                                         ^  |
              aux_srcs (read) -----------+  +-----------> aux_dsts (write)
                                         |
              aux_transformations -------+
              (called via transform())
```

1. The main pipeline **input value** arrives at the script step (after any
   preceding transform steps such as `clamp` or `ema`).
2. The script can **read** the main input via `read(0)` and any auxiliary
   source via `read("<key>")` or `read(<index>)`.
3. Arbitrary **Luau code** executes — math, logic, state management, calls
   to `transform()` for sub-pipeline processing, etc.
4. The script **writes** the main output via `write(0, value)` and
   optionally writes to auxiliary destinations via `write("<key>", value)`
   or `write(<index>, value)`.
5. The (possibly modified) value continues down the pipeline to subsequent
   transform steps and ultimately to the mapping destination.

The step runs on **every tick** — both on user-input events and on idle clock
ticks (configurable via `global.idle_tick_rate`). Use the `is_idle()` API
function inside the script to distinguish between the two and implement
time-based behaviour (e.g. integration, decay, periodic signals).

> **Tip:** Because Luau global variables persist across ticks, you can
> implement integrators, low-pass filters, state machines, or any custom
> signal processing that would be awkward or impossible with the built-in
> transform steps alone. Use `os.clock()` for wall-clock timing and
> `is_idle()` / `base_rate()` for tick-aware logic.

> **Tip:** For simple reshaping tasks (curves, clamping, smoothing) prefer
> the built-in transform steps — they are faster and easier to configure.
> Reach for `script` when you need branching logic, multiple I/O channels,
> or algorithmic processing that the declarative pipeline cannot express.

---

## Per-Parameter Reference

All parameters below are fields of the `script:` mapping step in the YAML
configuration.

### `enabled` (bool, default: `true`)

Master on/off switch for the entire script step. When `false`, the input
value passes through unchanged and the script is **not** executed.

---

### `lang` (enum, default: `Luau`)

The scripting language to use. Currently only **`Luau`** is supported.
This field is optional and defaults to `Luau`; you may omit it entirely.

---

### `script` (string, default: `""`)

The Luau source code to execute on every tick. The code is compiled **once**
(lazily, on first execution) into a callable function and then invoked on
each subsequent tick. Compilation errors are logged and the step degrades to
a no-op.

The script body has access to five global API functions (see
[Script API Reference](#script-api-reference) below) and to the full Luau
standard library (`math`, `string`, `table`, `os.clock`, etc.).

**Statefulness:** Luau global variables persist between ticks. A variable
assigned on one tick retains its value on the next. This is the primary
mechanism for maintaining state (e.g. accumulators, previous values,
timers). On first tick, uninitialized globals are `nil` — guard against
this with `if x == nil then x = 0 end` or similar.

Multi-line scripts are best expressed with YAML block scalars:

```yaml
script: |-
  local now = os.clock()
  if last == nil then last = now end
  local dt = now - last
  -- ... your logic here ...
  last = now
```

---

### `output_interval` (interval \[from, to\], optional, default: none)

Overrides the **output interval metadata** of this transform step.

When omitted, the output interval is inherited from the input (i.e. the
step does not change the pipeline's interval tracking). Set this when your
script produces values in a different range than the input and you want
downstream steps (and the destination control) to see the correct interval.

Example:

```yaml
output_interval: [-100.0, 100.0]
```

---

### `output_relativity` (`Abs` | `Rel`, optional, default: none)

Overrides the **output relativity metadata** of this transform step.

When omitted, the output relativity is inherited from the input. Set this
when your script converts between relative and absolute semantics (e.g.
integrating a relative input into an absolute output).

Example:

```yaml
output_relativity: Abs
```

---

### `aux_srcs` (map or list, optional, default: `{}`)

_(YAML alias:_ `sources`_)_

A collection of **auxiliary data sources** that the script can read at
runtime via the `read()` API function. Each entry binds an external value
(a variable, a device control, or a static number) to a **key** that the
script uses to reference it.

Can be specified as either:

* A **map** with explicit string keys:

  ```yaml
  aux_srcs:
    ffb_x:
      source:
        dev: VJoy1
        ctl: "[Auto-created] Force Feedback X"
      remap_to_interval: [-1.0, 1.0]
    gain:
      source:
        value: 20.0
        range: [0.0, 50.0]
  ```

* A **list** (auto-numbered with 1-based indices `"1"`, `"2"`, …):

  ```yaml
  aux_srcs:
    - source:
        dev: VJoy1
        ctl: "[Auto-created] Force Feedback X"
      remap_to_interval: [-1.0, 1.0]
    - source:
        value: 20.0
        range: [0.0, 50.0]
  ```

Each entry supports the following sub-fields:

#### `aux_srcs.<key>.source` (a static value or an arbitrary source, required)

The data source to read from. Accepts any valid `static value or an arbitrary source`:

* A **device control** reference: `{ dev: <device>, ctl: <control> }`
* A **variable** reference: `{ var: <name> }`
* A **static value**: `{ value: <number>, range: [from, to] }`

#### `aux_srcs.<key>.remap_to_interval` (interval \[from, to\], optional)

When set, the raw value read from `source` is **remapped** from the
source's native interval to this interval before the script sees it.

For example, if the source is a force-feedback axis with native range
\[-32768, 32767\] and you set `remap_to_interval: [-1.0, 1.0]`, the script
will receive a normalized value in \[-1, +1\].

When omitted, the raw value is passed through as-is.

---

### `aux_dsts` (map or list, optional, default: `{}`)

_(YAML alias:_ `destinations`_)_

A collection of **auxiliary data destinations** that the script can write
to at runtime via the `write()` API function. Each entry binds an external
target (a variable or a device control) to a **key** that the script uses
to reference it.

Can be specified as a **map** or a **list**, exactly like `aux_srcs`.

Each entry supports the following sub-fields:

#### `aux_dsts.<key>.destination` (a dynamic value destination, required)

The target to write to. Accepts any valid `dynamic value destination`:

* A **device control** reference: `{ dev: <device>, ctl: <control> }`
* A **variable** reference: `{ var: <name> }`
* `null` — a void destination (writes are silently discarded). Useful as
  a placeholder.

#### `aux_dsts.<key>.remap_from_interval` (interval \[from, to\], optional)

When set, the value written by the script is **remapped** from this
interval to the destination's native interval before being stored.

For example, if your script produces values in \[-100, 100\] and the
destination variable has a native range of \[-14000, 14000\], set
`remap_from_interval: [-100.0, 100.0]` and the framework will scale
automatically.

When omitted, the value is written as-is (the script is responsible for
producing values in the destination's native range).

---

### `aux_transformations` (map or list, optional, default: `{}`)

A collection of **named transformation sub-pipelines** that the script can
invoke by name via the `transform(name, value)` API function.

Each key maps to a standard transformation pipeline (a list of transform
steps), identical in format to the top-level `transformation:` field of a
mapping.

```yaml
aux_transformations:
  smooth_ff:
    - ema:
        enabled: true
        tau: 0.02
  shape_curve:
    - exp:
        enabled: true
        base: 2.5
        center_symmetric: true
```

From the script:

```lua
local smoothed = transform("smooth_ff", raw_force)
local shaped   = transform("shape_curve", smoothed)
```

Each sub-pipeline maintains its own independent execution state (filter
history, etc.), so calling the same named pipeline from multiple script
steps or mappings will **not** share state.

---

## Script API Reference

The following global functions are injected into the Luau environment
**before each tick** and are available for the duration of that tick's
execution. They are removed after the script returns.

### `read(key)` → number

Reads a value from an auxiliary source (or the main pipeline input).

| `key` | Meaning |
|-------|---------|
| `0` | The **main pipeline input** value (the value that entered this script step from the preceding transform or the mapping source). |
| `"<name>"` | The auxiliary source registered under the string key `<name>` in `aux_srcs`. |
| `<n>` (number ≥ 1) | The *n*-th auxiliary source (1-based insertion order) in `aux_srcs`. |

If `remap_to_interval` was configured for the source, the returned value
is already remapped. Returns the raw value otherwise.

Raises a Luau runtime error if the key is not found.

---

### `write(key, value)`

Writes a value to an auxiliary destination (or the main pipeline output).

| `key` | Meaning |
|-------|---------|
| `0` | The **main pipeline output** — overwrites the value that will be passed to the next transform step (or to the mapping destination if this is the last step). |
| `"<name>"` | The auxiliary destination registered under the string key `<name>` in `aux_dsts`. |
| `<n>` (number ≥ 1) | The *n*-th auxiliary destination (1-based insertion order) in `aux_dsts`. |

If `remap_from_interval` was configured for the destination, the value is
remapped before being written.

Raises a Luau runtime error if the key is not found.

> **Important:** If your script does not call `write(0, ...)`, the main
> pipeline output remains **unchanged** (the original input value passes
> through). You must explicitly write to index `0` to modify the main
> signal.

---

### `transform(name, value)` → number

Invokes a named sub-pipeline from `aux_transformations`.

* `name` — the string key of the sub-pipeline.
* `value` — the input value to feed into the pipeline.

Returns the pipeline's output value. The pipeline receives the value with
the interval and relativity metadata configured (or inferred) for that
sub-pipeline.

Raises a Luau runtime error if no transformation with the given name
exists.

---

### `is_idle()` → bool

Returns `true` if the current tick is an **idle tick** (no new user input
was received; the tick was generated by the idle clock at
`global.idle_tick_rate`). Returns `false` if the tick was triggered by an
actual input event.

Use this to implement time-dependent logic (decay, integration, periodic
signals) that should run continuously, vs. input-reactive logic that should
only run when the user moves a control.

---

### `base_rate()` → number

Returns the configured **idle tick rate** (in Hz) from
`global.idle_tick_rate`. Useful for computing `dt`-independent rates or
for verifying timing assumptions inside the script.

---

## Configuration Example

### Minimal example — passthrough with logging

```yaml
transformation:
  - script:
      enabled: true
      script: |-
        local v = read(0)
        if is_idle() then
          -- only log on idle ticks to avoid spam
        else
          print("input:", v)
        end
        write(0, v)   -- passthrough
```

### Using `aux_transformations` from a script

```yaml
transformation:
  - script:
      enabled: true
      script: |-
        local raw = read(0)
        local smoothed = transform("my_filter", raw)
        local shaped   = transform("my_curve", smoothed)
        write(0, shaped)
      aux_transformations:
        my_filter:
          - ema:
              enabled: true
              tau: 0.05
        my_curve:
          - exp:
              enabled: true
              base: 2.0
              center_symmetric: true
```
