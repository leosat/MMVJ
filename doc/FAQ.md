# MMVJ FAQ.

* [MMVJ FAQ.](#mmvj-faq)
  * [Controlling an imaginary car with mouse or MIDI controller - what a strange idea and how can it be even physically doable/ergonomic?](#controlling-an-imaginary-car-with-mouse-or-midi-controller---what-a-strange-idea-and-how-can-it-be-even-physically-doableergonomic)
  * [There are steering wheels and specialized controllers on the market, why to do it with mouse or a MIDI device?](#there-are-steering-wheels-and-specialized-controllers-on-the-market-why-to-do-it-with-mouse-or-a-midi-device)
  * [Configuration seems complex, where is Gui?](#configuration-seems-complex-how-to-enable-gui)
  * [I'm having hard time figuring out what all those numbers in config mean.](#im-having-hard-time-figuring-out-what-all-those-numbers-in-config-mean)
  * [Force feedback doesn't seem to work with my game and MMVJ, how to debug that?](#force-feedback-doesnt-seem-to-work-with-my-game-and-mmvj-how-to-debug-that)
  * [Force feedback works, but it's worse with it than without.](#force-feedback-works-but-its-worse-with-it-than-without)

---

## Controlling an imaginary car with mouse or MIDI controller - what a strange idea and how can it be even physically doable/ergonomic?

Firstly, **if we directly bind mouse movements to virtual joystick axis - and do only that - we will not be able to steer a well simulated vehicle**... It's very hard to find center position and in such a configuration the player will end up going left to right with amplitude or frequency increasing.

So how it may even work then? The answer: **there are some less obvious things which need to be addressed** and we do, see below.

For steering, our program utilizes force feedback coming from the game to emulate real steering wheel movement. As real life car (or bicycle or whatever...) does, the simulator will do 75% of work for you: the car is designed in such a way that wheels are coming in-line with the vehicle movement vector when under acceleration (self-alignment), this also results in natural steering wheel counter-rotation when a car is drifting (well, in most normal conditions). All of that is reported by the game to a game controller with force feedback. The controller reads force feedback and applies. The real steering wheel controler rumbles and rotates, the emulated one (our case) - can do just the same! With steering wheel doing most of the work "by itself", just like in real life, all that is required from the driver is to be ready and make corrections only when needed. So, you do not have to move mouse all the time trying to catch up with the road trajectory. The simulation and road profile will do most of it for you, just like with the real car driving.

In real life you are holding steering wheel with different firmness. When urgent correction required - you take it harder. When you see that the road profile and other factors make vehicle "self-steer" - you let go a bit. When going sideways and the wheel brutally counterrotates - you may also let it do it mostly itself (unless want your fingers to be harmed). To simulate this "hold or let go" behavior the application introduces "hold factor" that is handy to assign to mouse Y-axis movement ("vertical") whereas to assign left-right steering to mouse X-axis movement. See configuration examples.

For cases when force feedback is not available in the game or to augment it - there is autocentering with configurable dynamics provided (can be also turned off).

Autocenetering is applied whenever there's no active user input and no significant force feedback incoming. Both force feedback and autocenetering are decreased by the abovementioned "hold factor" - such that if you want to simulate that you are holding it firmly - the effect is intuitive - the steering wheel stays where you put it. Default config limits hold factor to prevent sudden "full lock", but that is totally configurable.

---

Pedals and steering can also be emulated with MIDI controllers. Pedals - with piano keys (think about different velocities corresponding to different amount of pedal press, add special filters to smooth-out discrete "note on" events and here you go, you have natural pedals action simulatied). Steering can also be naturally simulated with Pitch wheel which usually has mechanical centering spring acting as analog for auto-centering. You can also use midi expression pedals for... whatever you need. TODO: If your MIDI controllers are such good they have aftertouch or even per-note aftertoch - they are (to be soon) supported by the application.

## There are steering wheels and specialized controllers on the market, why to do it with mouse or a MIDI device?

When on the road, or on a budget, you need some other solution. And re-using what you already have is a good way to do it. The author owns a DD steering wheel and can confirm that while one may not exactly **feel** the feedback pushing your hands, the feedback still affects virtual steering wheel position, therefore your movements will correlate with the simulated vehicle behavior in a way "similar" to steering real car with real steering wheel (as much as it is at all approachable with just other type of movement). Being a flexible input-output mapping tool it's not only about simracing. You can create your setups for any types of usage including flight or other simulators. Flexible transformation/filtering pipelines will factilitate you with that.

Several more practical reasons:

1. **Space**: Full wheel setups require dedicated desk/cockpit space and mounting. Mouse/MIDI fits your existing workspace.
2. **Experimentation**: MMVJ lets you experiment with custom input mappings and control schemes that commercial wheels don't offer. You can map multiple axes from different devices simultaneously.
3. **Existing hardware**: If you already have a high-quality MIDI controller for music production, MMVJ lets you repurpose it for gaming without buying dedicated hardware.
4. **Learning tool**: MMVJ is also useful for understanding vehicle dynamics, force feedback systems, and input processing pipelines - it's an educational project as much as a practical tool.

## Configuration seems complex, how to enable Gui?

Using --gui command line option. The Gui allows configuration modifications and creation of new ones, provides with runtime monitoring of every dynamically updated value in the pipeline, including runtime plotting.

## I'm having hard time figuring out what all those numbers in config mean.

The configuration reference is on the way (WIP). It will be accessible as both plain md files and via Gui on corresponding knobs.

* **For steering debugging**: steering indicator shows your current steering position and hold factor visually. The telemetry graphs show time-series data for any traced values.
* Use [jstest-jtk](https://github.com/Grumbel/jstest-gtk) to see inputs translate to joysitck controls.
* **Start with presets**: Use example configs and predefined control types as starting points. 
* **Tune one parameter at a time**: Change a single curve exponent or filter parameter, then observe the effect in the telemetry overlay. This makes cause-and-effect clear.
* **Understand the intervals and relativity**: for more details refer to the glossary section in main readme.

## Force feedback doesn't seem to work with my game and MMVJ, how to debug that?

If using with a game on Linux, which runs under Wine, please see relevant README.md section describing how to configure Wine. In short, it requires virtual joysticks to be recognized as DInput devices and not XInput. Since Wine is in active development itself, there may be more details case by case (including how you configure your virtual joysticks (number and types of controls, bus choice or hardware IDs used) or how Wine itself is built/configured).

Feel free to discuss particular problematic cases and solutions in our community section!

## Force feedback works, but it's worse with it than without.

There are several possible reasons for that:

1. The force feedback coming from the game is in **"wrong" direction**. If you **disable autocentering** by setting it's halflife to 0 you will see that force feedback in such a case will not work to center the wheels while accelerating, instead working the opposite direction: working with your turns, not against them as it properly should (most of the time). There's special steering telemetry graphing GUI, which can be enabled at runtime. In it, in such erroneous cases, you shall see that FFB (the green curve) will point exactly the direction of steering (the blue/magenta/yellow curves), whereas in proper case it should point (in most cases under acceleration) the opposite direction.
   1. **Solution**: invert the force feedback either in the game or in the configuration file.
2. When FFB enabled it's influence is (a) **too harsh/clunky/brutal** - I have to move mouse excessively to accomodate that. Or, **on the opposite** (b) force feedback **influence is too low**, car feels uncontrollable (especially you can feel this with RWD or AWD ones).
   1. **Solution**: 
      1. **Use** steering transform **telemetry graph** to debug what's happening.
      2. In case of (a) **lower the gain** (if it was at 1.0, set it to 0.5 or even lower) or in case of (b) **increase the gain (can go > 1.0... maybe even 2.0 or so)**. Also see the in-game FFB parameters. Sometimes you have the sensitivity setting there acting as a compressor, increasing lower signals and squashing the high amplitude ones. Make sure you use little of that compression as if you'd do with a fine DD steering wheel.
      3. Try **using filtering** on FFB, see configuration examples.
         1. **E.g.** adding a bit of low-pass filter gives more smoothing but adds some lag in FF response.
      4. **Update rate**: After checking the above check the `idle_tick_update_rate` setting. While FFB is applied on both idle tick and with active user input (where rate is defined by the user input activity), this setting will affect behavior only when user makes no input. Try increasing to 100-250 Hz to see if that helps, but in my testing 60Hz is totally ok.

---
