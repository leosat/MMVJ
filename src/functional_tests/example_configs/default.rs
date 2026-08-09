#[cfg(test)]
mod default_config_tests {
    use crate::base_num::{BaseAtomicT, BaseNumT};
    use crate::config::ConfigManager;
    use crate::debug::DebugLevel;
    use crate::device_and_device_manager::{
        AvailableDeviceInfoIface, DeviceEvent, DeviceKind, DeviceManagerCommon, DeviceManagerWithFfb,
        OpenedDeviceInfoIface,
    };
    use crate::hid_device::{HidDeviceEvent, HidEvent};
    use crate::interner::intern_str;
    use crate::mapped_controls::MappedCtls;
    use crate::mapping::{MappedHidManager, Mapper, MappingEngine};
    use crate::num_interval::NumInterval;
    use crate::schemas_cfg::Config;
    use crate::schemas_common::ObjId;
    use crate::schemas_hid::HidDeviceCfg;
    use crate::schemas_transform::TfmStepCfg;
    use crate::schemas_value::WithNumericValueSettable;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering::Relaxed;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[cfg(feature = "midi")]
    use crate::mapping::MappedMidiManager;
    #[cfg(feature = "midi")]
    use crate::midi::MidiDeviceEvent;
    #[cfg(feature = "midi")]
    use crate::schemas_midi::MidiMatcherCfg;

    // TODO: make a reusable framework with sane API...
    // ---------------
    static TESTED_JOYSTICK_AXIS_MIDPOINT: BaseAtomicT = BaseAtomicT::new(0.0);
    static TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY: &str = "ABS_X";
    static TESTED_JOYSTICK_DEVICE_NAME: &str = "Virtual steering wheel";
    // ---------------
    static MOCKED_HID_DEVICE_NAME: &str = "Mock Mouse";
    // ---------------
    static AUTOCENTERING_TOLERANCE: BaseNumT = 0.07;
    static MAPPING_ENGINE_IDLE_RATE: u32 = 60;

    #[derive(Debug, Clone)]
    struct MockAvailableDevice {
        name: String,
        classification: enumflags2::BitFlags<DeviceKind>,
    }

    impl AvailableDeviceInfoIface for MockAvailableDevice {
        fn get_name(&self) -> &str {
            &self.name
        }
        fn get_classification(&self) -> enumflags2::BitFlags<DeviceKind> {
            self.classification
        }
    }

    #[derive(Debug, Clone)]
    struct MockOpenedDevice {
        id: ObjId,
        info: MockAvailableDevice,
    }

    impl OpenedDeviceInfoIface for MockOpenedDevice {
        fn get_opened_device_id(&self) -> ObjId {
            self.id
        }
        fn get_available_device_info(&self) -> &impl AvailableDeviceInfoIface {
            &self.info
        }
    }

    struct MockHidManager {
        event_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<HidDeviceEvent>>,
        outputs: Arc<Mutex<HashMap<(String, String), BaseNumT>>>,
        ff_spring_enabled: Arc<AtomicBool>,
    }

    impl MockHidManager {
        fn new(
            ff_spring_enabled: Arc<AtomicBool>,
        ) -> (
            Self,
            mpsc::UnboundedSender<HidDeviceEvent>,
            Arc<Mutex<HashMap<(String, String), BaseNumT>>>,
        ) {
            let (tx, rx) = mpsc::unbounded_channel();
            let outputs = Arc::new(Mutex::new(HashMap::new()));
            (
                Self {
                    event_rx: tokio::sync::Mutex::new(rx),
                    outputs: outputs.clone(),
                    ff_spring_enabled,
                },
                tx,
                outputs,
            )
        }
    }

    impl DeviceManagerCommon for MockHidManager {
        type AvailableDeviceInfoT = MockAvailableDevice;
        type DeviceCfgT = HidDeviceCfg;
        type DeviceKindFilterT = enumflags2::BitFlags<DeviceKind>;
        type DeviceEventT = HidDeviceEvent;
        type OpenedDeviceInfoT = MockOpenedDevice;

        fn open_device(
            &self,
            device_info: &Self::AvailableDeviceInfoT,
            _device_matcher_key: &str,
            _device_cfg: &Self::DeviceCfgT,
        ) -> anyhow::Result<Self::OpenedDeviceInfoT> {
            Ok(MockOpenedDevice {
                id: ObjId::from(intern_str(device_info.get_name())),
                info: device_info.clone(),
            })
        }

        async fn consume_any_opened_device_event(&self) -> Option<Self::DeviceEventT> {
            let mut rx = self.event_rx.lock().await;
            rx.recv().await
        }

        async fn device_monitor(
            &self,
            _match_name_regex: &regex::Regex,
            _filter: Option<Self::DeviceKindFilterT>,
        ) -> anyhow::Result<()> {
            std::future::pending::<()>().await;
            Ok(())
        }

        fn enumerate_available_devices(
            &self,
            _filter: Option<Self::DeviceKindFilterT>,
        ) -> Vec<Self::AvailableDeviceInfoT> {
            vec![
                MockAvailableDevice {
                    name: MOCKED_HID_DEVICE_NAME.to_string(),
                    classification: enumflags2::BitFlags::from_flag(DeviceKind::Mouse),
                },
                MockAvailableDevice {
                    name: "Mock Keyboard".to_string(),
                    classification: enumflags2::BitFlags::from_flag(DeviceKind::Keyboard),
                },
            ]
        }

        fn set_control_matcher_and_broadcast(&self, dev_key: &str, ctl_key: &str, value: BaseNumT, _silent: bool) {
            self.outputs
                .lock()
                .unwrap()
                .insert((dev_key.to_string(), ctl_key.to_string()), value);
        }

        fn stop(&self, _full_shutdown: bool) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl DeviceManagerWithFfb for MockHidManager {
        fn ff_set_x_axis_pos(&self, _dev_key: &str, _ctl_key: &str, _control_interval: NumInterval<BaseNumT>) {}
        fn ff_set_y_axis_pos(&self, _dev_key: &str, _ctl_key: &str, _control_interval: NumInterval<BaseNumT>) {}
        fn ff_get_x_sum_symm_norm(&self, _dev_key: &str) -> BaseNumT {
            if !self.ff_spring_enabled.load(Ordering::Relaxed) {
                return 0.0;
            }
            let current_abs_x = self
                .outputs
                .lock()
                .unwrap()
                .get(&(
                    TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                    TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                ))
                .copied()
                .unwrap_or(0.0);

            // Emulate a very strong centering spring
            if current_abs_x > TESTED_JOYSTICK_AXIS_MIDPOINT.load(Relaxed) {
                -100.0 // * current_abs_x.abs()
            } else if current_abs_x < TESTED_JOYSTICK_AXIS_MIDPOINT.load(Relaxed) {
                100.0 // * current_abs_x.abs()
            } else {
                0.0
            }
        }
        fn ff_get_y_sum_symm_norm(&self, _dev_key: &str) -> BaseNumT {
            0.0
        }
    }

    impl MappedHidManager for MockHidManager {}

    #[cfg(feature = "midi")]
    struct MockMidiManager;

    #[cfg(feature = "midi")]
    impl DeviceManagerCommon for MockMidiManager {
        type AvailableDeviceInfoT = MockAvailableDevice;
        type DeviceCfgT = MidiMatcherCfg;
        type DeviceKindFilterT = enumflags2::BitFlags<DeviceKind>;
        type DeviceEventT = MidiDeviceEvent;
        type OpenedDeviceInfoT = MockOpenedDevice;

        fn open_device(
            &self,
            device_info: &Self::AvailableDeviceInfoT,
            device_matcher_key: &str,
            _device_cfg: &Self::DeviceCfgT,
        ) -> anyhow::Result<Self::OpenedDeviceInfoT> {
            Ok(MockOpenedDevice {
                id: ObjId::from(intern_str(device_matcher_key)),
                info: device_info.clone(),
            })
        }
        async fn consume_any_opened_device_event(&self) -> Option<Self::DeviceEventT> {
            std::future::pending::<Option<Self::DeviceEventT>>().await
        }
        async fn device_monitor(
            &self,
            _match_name_regex: &regex::Regex,
            _filter: Option<Self::DeviceKindFilterT>,
        ) -> anyhow::Result<()> {
            std::future::pending::<()>().await;
            Ok(())
        }
        fn enumerate_available_devices(
            &self,
            _filter: Option<Self::DeviceKindFilterT>,
        ) -> Vec<Self::AvailableDeviceInfoT> {
            vec![]
        }
        fn set_control_matcher_and_broadcast(&self, _dev_key: &str, _ctl_key: &str, _value: BaseNumT, _silent: bool) {}
        fn stop(&self, _full_shutdown: bool) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[cfg(feature = "midi")]
    impl MappedMidiManager for MockMidiManager {}

    // --- Test Scenario Helper Functions ---

    pub(crate) struct Period(pub(crate) u64);
    pub(crate) struct Count(pub(crate) usize);

    async fn send_rel(
        tx: &mpsc::UnboundedSender<HidDeviceEvent>,
        mouse_id: ObjId,
        rel: MappedCtls,
        val: BaseNumT,
        count: Count,
        period: Period,
    ) {
        for _ in 0..count.0 {
            let _ = tx.send(DeviceEvent {
                device_id: mouse_id,
                data: HidEvent {
                    control_type: rel,
                    value: val,
                },
            });
            tokio::time::sleep(Duration::from_millis(period.0)).await;
        }
    }

    async fn set_hold_factor_max(tx: &mpsc::UnboundedSender<HidDeviceEvent>, mouse_id: ObjId) {
        send_rel(tx, mouse_id, MappedCtls::RelY, 127.0, Count(20), Period(10)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    async fn set_hold_factor_zero(tx: &mpsc::UnboundedSender<HidDeviceEvent>, mouse_id: ObjId) {
        send_rel(tx, mouse_id, MappedCtls::RelY, -127.0, Count(40), Period(10)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    fn disable_autocentering(cfg: &mut Config) {
        for mapping in &mut cfg.mappings {
            for step in &mut mapping.transformation.steps {
                if let TfmStepCfg::Steering(s) = step {
                    s.auto_center_halflife.set_numeric_value(0.0);
                }
            }
        }
    }

    async fn run_engine_and_events<F>(cfg: Config, ff_spring_enabled: Arc<AtomicBool>, scenario: F)
    where
        F: FnOnce(
            Arc<Mutex<HashMap<(String, String), BaseNumT>>>,
            mpsc::UnboundedSender<HidDeviceEvent>,
            Arc<AtomicBool>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
    {
        let lua = mlua::Lua::new();
        let (mock_hid_mgr, event_tx, outputs) = MockHidManager::new(ff_spring_enabled.clone());

        #[cfg(feature = "midi")]
        let mock_midi_mgr = MockMidiManager;

        #[cfg(feature = "midi")]
        let mut engine: MappingEngine<MockHidManager, MockMidiManager> = MappingEngine::new(
            DebugLevel::Mid,
            true,
            cfg,
            &mock_hid_mgr,
            #[cfg(feature = "midi")]
            &mock_midi_mgr,
            &lua,
        )
        .expect("Failed to create engine");

        #[cfg(not(feature = "midi"))]
        let mut engine: MappingEngine<MockHidManager, ()> = MappingEngine::new(
            DebugLevel::Low,
            false,
            cfg,
            &mock_hid_mgr,
            #[cfg(feature = "midi")]
            &mock_midi_mgr,
            &lua,
        )
        .expect("Failed to create engine");

        engine.set_idle_tick_rate(MAPPING_ENGINE_IDLE_RATE);
        engine.init().expect("Failed to init engine");

        let scenario_fut = scenario(outputs.clone(), event_tx, ff_spring_enabled.clone());

        tokio::select! {
            _ = engine.run() => { println!(" ENGINE RUN COMPLETED FIRST \n\n"); },
            _ = scenario_fut => { println!(" SCENARIO COMPLETED \n\n"); },
            _ = tokio::time::sleep(Duration::from_secs(10)) => { println!(" TIMEOUT! \n\n");},
        }

        engine.stop().expect("Failed to stop engine");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_default_config_extended() {
        let cfg_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conf/example-default.yaml");
        let mut cfg_mgr = ConfigManager::new(&cfg_path, DebugLevel::Mid).unwrap();
        cfg_mgr.load().unwrap();

        let cfg_base = cfg_mgr.cfg_ref().clone();

        let ff_spring_enabled = Arc::new(AtomicBool::new(false));

        let joystick_abs_x_range = cfg_base
            .devices
            .hid
            .get(TESTED_JOYSTICK_DEVICE_NAME)
            .unwrap()
            .controls
            .get(TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY)
            .unwrap()
            .range;

        TESTED_JOYSTICK_AXIS_MIDPOINT.store(joystick_abs_x_range.midpoint() as BaseNumT, Relaxed);

        env_logger::builder().is_test(true).try_init().unwrap();

        // --- Call 1: Phases 0, 1, 2 ---
        {
            let cfg = cfg_base.clone();
            let scenario = |outputs: Arc<Mutex<HashMap<(String, String), BaseNumT>>>,
                            event_tx: mpsc::UnboundedSender<HidDeviceEvent>,
                            ff_enable_flag: Arc<AtomicBool>|
             -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>> {
                Box::pin(async move {
                    let mouse_id = ObjId::from(intern_str(MOCKED_HID_DEVICE_NAME));

                    // 0) Check axis turning opposit direction
                    set_hold_factor_max(&event_tx, mouse_id).await;
                    send_rel(&event_tx, mouse_id, MappedCtls::RelX, 50.0, Count(15), Period(10)).await;
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    let val_right = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);

                    assert!(
                        val_right > joystick_abs_x_range.midpoint(),
                        "0) Expected right turn, got {}",
                        val_right
                    );

                    send_rel(&event_tx, mouse_id, MappedCtls::RelX, -50.0, Count(30), Period(1)).await;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let val_left = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);
                    assert!(
                        val_left < joystick_abs_x_range.midpoint(),
                        "0) Expected left turn, got {}",
                        val_left
                    );
                    println!(
                        "0) Opposite directions passed: right={:.3}, left={:.3}",
                        val_right, val_left
                    );

                    // 1) Autocentering (as configured)
                    set_hold_factor_max(&event_tx, mouse_id).await;
                    send_rel(&event_tx, mouse_id, MappedCtls::RelX, 50.0, Count(20), Period(0)).await;
                    //tokio::time::sleep(Duration::from_millis(100)).await;

                    let val_before = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);
                    assert!(
                        val_before > joystick_abs_x_range.midpoint(),
                        "1) Wheel must be turned right, got {}",
                        val_before
                    );

                    set_hold_factor_zero(&event_tx, mouse_id).await;
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                    let val_after = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);
                    assert!(
                        val_after.trunc() == joystick_abs_x_range.midpoint().trunc(),
                        "1) Wheel must have autocentered. Expected Midpoint {:.3}, Before: {:.3}, After: {:.3}",
                        joystick_abs_x_range.midpoint(),
                        val_before,
                        val_after
                    );
                    println!("1) Autocentering passed: ABS_X = {:.3}", val_after);

                    // 2) FF spring (as configured)
                    ff_enable_flag.store(true, Ordering::Relaxed);

                    set_hold_factor_max(&event_tx, mouse_id).await;
                    send_rel(&event_tx, mouse_id, MappedCtls::RelX, 50.0, Count(20), Period(0)).await;
                    //tokio::time::sleep(Duration::from_millis(100)).await;

                    let val_before = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);
                    assert!(
                        val_before > joystick_abs_x_range.midpoint(),
                        "2) Wheel must be turned right, got {}",
                        val_before
                    );

                    set_hold_factor_zero(&event_tx, mouse_id).await;
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                    let val_after = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);
                    assert!(
                        (val_after.abs() - joystick_abs_x_range.midpoint().abs()).abs()
                            < joystick_abs_x_range.span() * AUTOCENTERING_TOLERANCE,
                        "2) FFB must have centered wheel. Expected Midpoint: {:.3} Before: {:.3}, After: {:.3}",
                        joystick_abs_x_range.midpoint(),
                        val_before,
                        val_after
                    );
                    println!("2) FF Spring passed: ABS_X = {:.3}", val_after);
                })
            };

            run_engine_and_events(cfg, ff_spring_enabled.clone(), scenario).await;
        }

        // --- Call 2: Phase 3 ---
        {
            let mut cfg = cfg_base.clone();
            disable_autocentering(&mut cfg);
            ff_spring_enabled.store(false, Ordering::Relaxed); // Reset flag for new engine

            let scenario = |outputs: Arc<Mutex<HashMap<(String, String), BaseNumT>>>,
                            event_tx: mpsc::UnboundedSender<HidDeviceEvent>,
                            ff_enable_flag: Arc<AtomicBool>|
             -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>> {
                Box::pin(async move {
                    let mouse_id = ObjId::from(intern_str("any_mouse"));

                    // 3) Disable autocentering and check FF spring
                    ff_enable_flag.store(true, Ordering::Relaxed);

                    set_hold_factor_max(&event_tx, mouse_id).await;
                    send_rel(&event_tx, mouse_id, MappedCtls::RelX, 50.0, Count(20), Period(0)).await;
                    //tokio::time::sleep(Duration::from_millis(100)).await;

                    let val_before = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);
                    assert!(
                        val_before > joystick_abs_x_range.midpoint(),
                        "3) Wheel must be turned right, got {}",
                        val_before
                    );

                    set_hold_factor_zero(&event_tx, mouse_id).await;
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                    let val_after = outputs
                        .lock()
                        .unwrap()
                        .get(&(
                            TESTED_JOYSTICK_DEVICE_NAME.to_string(),
                            TESTED_JOYSTICK_AXIS_CTL_MATCHER_KEY.to_string(),
                        ))
                        .copied()
                        .unwrap_or(0.0);
                    assert!(
                        (val_after.abs() - joystick_abs_x_range.midpoint().abs()).abs()
                            < joystick_abs_x_range.span() * AUTOCENTERING_TOLERANCE,
                        "3) FFB must have centered wheel without autocentering. Before: {:.3}, After: {:.3}",
                        val_before,
                        val_after
                    );
                    println!("3) FF Spring (no autocenter) passed: ABS_X = {:.3}", val_after);
                })
            };

            run_engine_and_events(cfg, ff_spring_enabled.clone(), scenario).await;
        }
    }
}
