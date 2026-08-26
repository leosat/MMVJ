#[cfg(test)]
mod steering_tfm_deserialization_and_exec_tests {
    use crate::base_num::BaseNumT;
    use crate::num_interval::NumInterval;
    use crate::num_interval::SYMM_UNIT_INTERVAL;
    use crate::relativity::Relativity;
    use crate::schemas_transform::{TfmSeqCfg, TfmStepCfg};
    use crate::schemas_value::AutoOrManual;
    use crate::schemas_value::DeviceControlMatcherRef;
    use crate::schemas_value::ValueDsts;
    use crate::schemas_value::{InputValueMetadata, TfmValue};
    use crate::schemas_value_port::ValuePortIface;
    use crate::tfm_exec::TfmExeState;
    use crate::tfm_exec::{TfmExecCtx, WithTfmExec};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    struct MockCtx {
        is_idle: bool,
        main_dst: ValueDsts,
        dyn_values: RefCell<HashMap<String, BaseNumT>>,
        ff_x: BaseNumT,
        ff_y: BaseNumT,
    }

    impl MockCtx {
        fn active() -> Self {
            Self {
                is_idle: false,
                main_dst: ValueDsts::Void(None),
                dyn_values: Default::default(),
                ff_x: 0.0,
                ff_y: 0.0,
            }
        }

        fn idle() -> Self {
            Self {
                is_idle: true,
                ..Self::active()
            }
        }

        fn get_written(&self, key: &str) -> Option<BaseNumT> {
            self.dyn_values.borrow().get(key).copied()
        }
    }

    impl TfmExecCtx for MockCtx {
        fn is_idle_tick(&self) -> bool {
            self.is_idle
        }

        fn get_idle_tick_rate(&self) -> u32 {
            100
        }

        fn get_main_dst(&self) -> Option<&ValueDsts> {
            Some(&self.main_dst)
        }

        fn device_control_matcher_ref_write(&self, dcm: &DeviceControlMatcherRef, v: BaseNumT) {
            self.dyn_values
                .borrow_mut()
                .insert(format!("{}.{}", dcm.device_matcher_key, dcm.control_matcher_key), v);
        }

        fn get_ff_x(&self, _dk: &str) -> BaseNumT {
            self.ff_x
        }

        fn get_ff_y(&self, _dk: &str) -> BaseNumT {
            self.ff_y
        }
    }

    /// Simulate a mouse REL_X event (range [-127, 127], relative).
    fn mouse_rel_x(value: BaseNumT) -> TfmValue<BaseNumT> {
        TfmValue {
            value,
            interval: NumInterval::new(-127.0, 127.0),
            relativity: Relativity::Rel,
        }
    }

    /// Simulate a zero-input idle tick.
    fn idle_input() -> TfmValue<BaseNumT> {
        TfmValue {
            value: 0.0,
            interval: NumInterval::new(-127.0, 127.0),
            relativity: Relativity::Rel,
        }
    }

    /// Set up pipeline metadata the same way `Mapping::recompute_metadata`
    /// does for a mouse REL_X source.
    fn setup_mouse_metadata(tfm: &mut TfmSeqCfg) {
        let _ = tfm.recompute_steps_metadata_get_out_interval_and_relativity(AutoOrManual::Auto(InputValueMetadata {
            interval: NumInterval::new(-127.0, 127.0),
            relativity: Relativity::Rel,
        }));
    }

    /// Rewind the steering step's internal clock by `dt` so the next
    /// `exec()` sees a realistic time delta **without sleeping**.
    fn rewind_steering_clock(tfm: &TfmSeqCfg, dt: Duration) {
        for step in &tfm.steps {
            if let TfmStepCfg::Steering(s) = step {
                s.exe_state_mut().last_time = Instant::now() - dt;
            }
        }
    }

    const EPSILON: BaseNumT = 1e-4;

    fn approx(a: BaseNumT, b: BaseNumT) -> bool {
        (a - b).abs() < EPSILON
    }

    const DEFAULT_YAML_STEERING_PIPELINE: &str = r#"
- ema:
    enabled: true
    tau: 0.04
- steering:
    enabled: true
    deadzone_counts: 0.0
    input_gain: 0.1
    auto_center_halflife: 0.15
    auto_center_along_force_feedback: 0.0
    hold_factor: 0.0
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
      - exp:
          enabled: true
          base: 3.3
          center_symmetric: true
"#;

    #[test]
    fn test_deserialize_default_yaml_steering_pipeline() {
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(DEFAULT_YAML_STEERING_PIPELINE)
            .expect("default.yaml steering pipeline must deserialize");

        assert_eq!(tfm.steps.len(), 2, "expected [ema, steering]");
        assert!(matches!(&tfm.steps[0], TfmStepCfg::Ema(_)), "step 0 = EMA");
        assert!(matches!(&tfm.steps[1], TfmStepCfg::Steering(_)), "step 1 = Steering");

        if let TfmStepCfg::Steering(s) = &tfm.steps[1] {
            assert!(*s.enabled);
            assert!(approx(s.input_gain.port_get_numeric_value(None::<&()>), 0.1));
            assert!(approx(s.auto_center_halflife.port_get_numeric_value(None::<&()>), 0.15));

            let ff = s.force_feedback.as_ref().expect("FFB must be present");
            assert!(*ff.enabled);
            assert!(approx(ff.gain.port_get_numeric_value(None::<&()>), 1.0));
            assert!(!ff.invert);
            assert_eq!(ff.transformation.steps.len(), 1, "FF sub-pipeline: 1 EMA");

            assert_eq!(
                s.integrated_user_input_transform.steps.len(),
                1,
                "user-input sub-pipeline: 1 exp curve"
            );
        }

        setup_mouse_metadata(&mut tfm);

        let ctx = MockCtx::active();
        let out = tfm.exec(mouse_rel_x(50.0), &ctx);

        assert_eq!(out.interval, SYMM_UNIT_INTERVAL, "output interval = [-1, 1]");
        assert_eq!(out.relativity, Relativity::Abs, "steering output is absolute");
        assert!(out.value > 0.0, "rightward mouse → positive wheel angle");
        assert!(out.value <= 1.0, "clamped to [-1, 1]");

        println!("default.yaml pipeline: REL_X=50 -> wheel={:.6}", out.value);
    }

    #[test]
    fn test_steering_accumulation() {
        let yaml = r#"
- steering:
    enabled: true
    input_gain: 0.2
    auto_center_halflife: 0.0
    hold_factor: 0.0
"#;
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm);
        let ctx = MockCtx::active();

        let mut prev = 0.0;
        for i in 1..=5 {
            let out = tfm.exec(mouse_rel_x(30.0), &ctx);
            assert!(out.value > prev, "tick {i}: must increase");
            prev = out.value;
        }
        for _ in 0..5 {
            let out = tfm.exec(mouse_rel_x(-30.0), &ctx);
            assert!(out.value < prev, "leftward must decrease");
            prev = out.value;
        }

        println!("Accumulation: 5*right then 5*left, final={prev:.6}");
    }

    #[test]
    fn test_steering_autocentering() {
        let yaml = r#"
- steering:
    enabled: true
    input_gain: 0.5
    auto_center_halflife: 0.3
    hold_factor: 0.0
"#;
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm);

        let turned = tfm.exec(mouse_rel_x(100.0), &MockCtx::active()).value;
        assert!(turned > 0.1, "must have turned, got {turned}");

        let ctx_idle = MockCtx::idle();
        let mut pos = turned;
        for _ in 0..20 {
            rewind_steering_clock(&tfm, Duration::from_millis(100));
            let out = tfm.exec(idle_input(), &ctx_idle);
            assert!(out.value < pos, "autocentering must reduce |pos|");
            pos = out.value;
        }

        assert!(
            pos.abs() < turned.abs() * 0.1,
            "after 2 s the wheel must be near zero, got {pos}"
        );
        println!("Autocentering: {turned:.4} → {pos:.6} after 2 s idle");
    }

    #[test]
    fn test_steering_no_autocentering_when_halflife_zero() {
        let yaml = r#"
- steering:
    enabled: true
    input_gain: 0.5
    auto_center_halflife: 0.0
    hold_factor: 0.0
"#;
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm);

        let turned = tfm.exec(mouse_rel_x(100.0), &MockCtx::active()).value;

        let ctx_idle = MockCtx::idle();
        for _ in 0..10 {
            rewind_steering_clock(&tfm, Duration::from_millis(100));
            let out = tfm.exec(idle_input(), &ctx_idle);
            assert!(approx(out.value, turned), "halflife=0 → wheel must stay");
        }
        println!("halflife=0: stayed at {turned:.6}");
    }

    #[test]
    fn test_steering_hold_factor_suppresses_autocentering() {
        let yaml = r#"
- steering:
    enabled: true
    input_gain: 0.5
    auto_center_halflife: 0.3
    hold_factor: 1.0
"#;
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm);

        let turned = tfm.exec(mouse_rel_x(100.0), &MockCtx::active()).value;

        let ctx_idle = MockCtx::idle();
        for _ in 0..10 {
            rewind_steering_clock(&tfm, Duration::from_millis(100));
            let out = tfm.exec(idle_input(), &ctx_idle);
            assert!(approx(out.value, turned), "hold=1.0 → locked");
        }
        println!("hold_factor=1.0: locked at {turned:.6}");
    }

    #[test]
    fn test_steering_input_gain_scaling() {
        let make = |gain: BaseNumT| -> TfmSeqCfg {
            let yaml = format!(
                r#"
- steering:
    enabled: true
    input_gain: {gain}
    auto_center_halflife: 0.0
    hold_factor: 0.0
"#
            );
            let mut t: TfmSeqCfg = serde_saphyr::from_str(&yaml).unwrap();
            setup_mouse_metadata(&mut t);
            t
        };

        let ctx = MockCtx::active();
        let lo = make(0.05).exec(mouse_rel_x(50.0), &ctx).value;
        let hi = make(0.50).exec(mouse_rel_x(50.0), &ctx).value;

        assert!(hi > lo, "higher gain -> larger movement");
        println!("Input_gain: 0.05 -> {lo:.6}, 0.50 -> {hi:.6}");
    }

    #[test]
    fn test_steering_exp_curve_reduces_center_sensitivity() {
        let yaml_curved = r#"
- steering:
    enabled: true
    input_gain: 0.3
    auto_center_halflife: 0.0
    hold_factor: 0.0
    integrated_user_input_transform:
      - exp:
          enabled: true
          base: 3.3
          center_symmetric: true
"#;
        let yaml_linear = r#"
- steering:
    enabled: true
    input_gain: 0.3
    auto_center_halflife: 0.0
    hold_factor: 0.0
"#;
        let mut curved: TfmSeqCfg = serde_saphyr::from_str(yaml_curved).unwrap();
        let mut linear: TfmSeqCfg = serde_saphyr::from_str(yaml_linear).unwrap();
        setup_mouse_metadata(&mut curved);
        setup_mouse_metadata(&mut linear);

        let ctx = MockCtx::active();
        let c = curved.exec(mouse_rel_x(10.0), &ctx).value;
        let l = linear.exec(mouse_rel_x(10.0), &ctx).value;

        assert!(c.abs() < l.abs(), "exp curve must attenuate center");
        println!("Exp curve: curved={c:.6} < linear={l:.6}");
    }

    #[test]
    fn test_steering_ffb_custom_source() {
        let yaml = r#"
- steering:
    enabled: true
    input_gain: 0.1
    auto_center_halflife: 0.0
    hold_factor: 0.0
    force_feedback:
      enabled: true
      gain: 1.0
      invert: false
      component: X
      custom_source:
        value: 0.8
        range: [-1.0, 1.0]
"#;
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm);

        // Tiny nudge so the wheel is not exactly at zero
        let _ = tfm.exec(mouse_rel_x(1.0), &MockCtx::active());

        // Idle tick with 500 ms simulated gap.
        // Expected FFB offset ≈ 0.8 * 1.0 * (1−0.0) * 0.5 = 0.4
        rewind_steering_clock(&tfm, Duration::from_millis(500));
        let out = tfm.exec(idle_input(), &MockCtx::idle());

        assert!(out.value > 0.05, "FFB must push wheel rightward");
        println!("FFB custom_source=0.8, dt=0.5 s → wheel={:.6}", out.value);
    }

    #[test]
    fn test_steering_ffb_invert() {
        let make = |invert: bool| -> TfmSeqCfg {
            let yaml = format!(
                r#"
- steering:
    enabled: true
    input_gain: 0.1
    auto_center_halflife: 0.0
    hold_factor: 0.0
    force_feedback:
      enabled: true
      gain: 1.0
      invert: {invert}
      custom_source:
        value: 0.5
        range: [-1.0, 1.0]
"#
            );
            let mut t: TfmSeqCfg = serde_saphyr::from_str(&yaml).unwrap();
            setup_mouse_metadata(&mut t);
            t
        };

        let normal = make(false);
        let inverted = make(true);

        let _ = normal.exec(mouse_rel_x(1.0), &MockCtx::active());
        let _ = inverted.exec(mouse_rel_x(1.0), &MockCtx::active());

        rewind_steering_clock(&normal, Duration::from_millis(500));
        rewind_steering_clock(&inverted, Duration::from_millis(500));

        let n = normal.exec(idle_input(), &MockCtx::idle()).value;
        let i = inverted.exec(idle_input(), &MockCtx::idle()).value;

        assert!(n > 0.0 && i < n, "inverted must push opposite");
        println!("FFB invert: normal={n:.6}, inverted={i:.6}");
    }

    #[test]
    #[ignore = "Debug failure"]
    fn test_steering_accumulator_variable() {
        let yaml = r#"
- steering:
    enabled: true
    input_gain: 0.2
    auto_center_halflife: 0.0
    hold_factor: 0.0
    accumulator:
      var: My Wheel Angle
"#;
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm);

        let ctx = MockCtx::active();
        let out = tfm.exec(mouse_rel_x(50.0), &ctx);

        let written = ctx
            .get_written("My Wheel Angle")
            .expect("accumulator must write to variable");
        assert!(written != 0.0, "persisted angle must be non-zero");

        println!("Accumulator: out={:.6}, persisted={:.6}", out.value, written);
    }

    #[test]
    fn test_steering_disabled_passthrough() {
        let yaml = r#"
- steering:
    enabled: false
    input_gain: 0.5
"#;
        let mut tfm: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm);

        let out = tfm.exec(mouse_rel_x(42.0), &MockCtx::active());
        assert!(approx(out.value, 42.0), "disabled → passthrough");
        println!("Disabled: 42.0 → {:.6}", out.value);
    }

    #[test]
    fn test_steering_yaml_roundtrip() {
        let yaml = r#"
- steering:
    enabled: true
    input_gain: 0.15
    auto_center_halflife: 0.25
    auto_center_along_force_feedback: 0.5
    hold_factor: 0.3
    force_feedback:
      enabled: true
      gain: 2.0
      component: 'Y'
      transformation:
        - ema:
            enabled: true
            tau: 0.02
    integrated_user_input_transform:
      - exp:
          enabled: true
          base: 2.0
          center_symmetric: true
"#;
        let mut tfm1: TfmSeqCfg = serde_saphyr::from_str(yaml).unwrap();
        setup_mouse_metadata(&mut tfm1);

        let serialized = serde_saphyr::to_string(&tfm1).expect("serialize");
        let mut tfm2: TfmSeqCfg = serde_saphyr::from_str(&serialized).expect("re-deserialize");
        setup_mouse_metadata(&mut tfm2);

        let ctx = MockCtx::active();
        let o1 = tfm1.exec(mouse_rel_x(60.0), &ctx).value;
        let o2 = tfm2.exec(mouse_rel_x(60.0), &ctx).value;

        assert!(approx(o1, o2), "round-trip must preserve behaviour");
        println!("Round-trip: both -> {o1:.6}");
    }

    //     #[test]
    //     fn test_steering_bool_shorthand() {
    //         for (literal, expected) in [("true", 1.0), ("false", 0.0)] {
    //             let yaml = format!(
    //                 r#"
    // - steering:
    //     enabled: true
    //     input_gain: 0.1
    //     auto_center_along_force_feedback: {literal}
    // "#
    //             );
    //             let tfm: TfmSeqCfg = serde_saphyr::from_str(&yaml).unwrap();
    //             if let TfmStepCfg::Steering(s) = &tfm.steps[0] {
    //                 assert!(
    //                     approx(s.auto_center_along_force_feedback.get_numeric_value(), expected),
    //                     "`{literal}` -> {expected}"
    //                 );
    //             }
    //         }
    //     }

    #[test]
    fn test_steering_input_gain_aliases() {
        for alias in ["smoothing_alpha", "input_sensitivity"] {
            let yaml = format!(
                r#"
- steering:
    enabled: true
    {alias}: 0.42
    auto_center_halflife: 0.0
"#
            );
            let tfm: TfmSeqCfg = serde_saphyr::from_str(&yaml).unwrap();
            if let TfmStepCfg::Steering(s) = &tfm.steps[0] {
                assert!(
                    approx(s.input_gain.port_get_numeric_value(None::<&()>), 0.42),
                    "alias `{alias}` -> input_gain"
                );
            }
        }
        println!("Aliases smoothing_alpha / input_sensitivity → input_gain");
    }
}
