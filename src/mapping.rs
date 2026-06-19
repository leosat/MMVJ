use crate::common::{
    BaseNumT, DeviceManager, Relativity, SYMM_UNIT_INTERVAL, SharedAtomicState, UNIT_INTERVAL, get_interned_str,
};
use crate::config::DebugLevel;
use crate::curves::Curves;
use crate::filters::OneEuroFilter;
#[cfg(feature = "gui")]
use crate::gui_transform_step::{self, TfmStepTraceStage};
use crate::hid_device::HidDeviceKind;
use crate::hid_manager::HidManager;
use crate::mapped_controls::MappedCtls;
use crate::mapped_device::{MappedDeviceEvent, MappedEvents, MappedHidEvent};
#[cfg(feature = "midi")]
use crate::midi::{MappedMidiMessage, MidiManager};
use crate::num_interval::{NumInterval, OutOfRangePolicy};
use crate::schemas_cfg::Config;
use crate::schemas_common::{ObjId, WithRuntimeId};
use crate::schemas_control_matcher::ControlMatchers;

use crate::schemas_mapping::Mapping;
use crate::schemas_transform::{
    DynValFilter, EmaFilterCfg, ForceFeedbackComponent, HighPassCfg, IntegrateCfg, LinearCfg, NormExpCfg,
    OneEuroFilterCfg, RaiseFallCfg, SCurveCfg, ScriptCfg, SignedPowerCfg, SteeringCfg, TfmSeqCfg, TfmStepCfg,
    collect_dynamic_value_matchers,
};
use crate::schemas_value::{DynValueRefs, ValueDsts, ValueSrcs, WithNumInterval};
use crate::schemas_value::{MappedValue, WithNumericValue};
use crate::schemas_value::{WithLastKnownIO, WithRelativity};
#[cfg(feature = "gui")]
use crate::tracing::GraphDisplayStyle;
use anyhow::{Context, Result, bail};
#[cfg(feature = "gui")]
use eframe::egui::Color32;
use log::{debug, info, warn};
use mlua::Lua;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::ops::Add;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;
use tokio::select;
use tokio::time::{Duration, MissedTickBehavior, interval};
use unchecked_refcell::{UncheckedRefCell, UncheckedRefMut};

#[derive(Clone)]
pub(crate) struct ScriptMappingState {
    #[allow(unused)]
    pub(crate) lua: Lua,
    pub(crate) inputs: mlua::Table,
    pub(crate) outputs: mlua::Table,
    pub(crate) compiled: mlua::Function,
}

#[derive(Clone, Copy)]
struct SteeringMappingState {
    last_time: Instant,
    pre_filter: BaseNumT,
    post_filter: BaseNumT,
}

struct RaiseFallMappingState {
    prev_out: BaseNumT,
    last_target: BaseNumT,
    prev_out_time: Option<Instant>,
    prev_user_input_time: Option<Instant>,
}

struct IntegrateMappingState {
    prev_val: BaseNumT,
}

pub(crate) struct MappingEngine<'driver_loop> {
    running: bool,
    idle_tick_rate: u32,
    // ---
    debug: DebugLevel,
    debug_idle_tick: bool,
    // ---
    cfg: Config,
    hid_mgr: &'driver_loop HidManager,
    #[cfg(feature = "midi")]
    midi_mgr: MidiManager,
    // ---
    _shared_atomic_state: &'driver_loop SharedAtomicState,
    // ---
    //  Mapping router algorithm index and runtime buffer.
    // ---
    router_index_sysdev_and_ctl_type_to_cms_and_mappings:
        HashMap<(ObjId, MappedCtls), (Vec<ControlMatchers>, Vec<usize>)>,
    router_buff_mappings_to_execute: Vec<usize>,
    // ---
    info_sysdev_to_enabled_mappings: HashMap<ObjId, Vec<usize>>, // NB: this is only used in mappings init routine, but leaving here for potential future use in other places.
    // ---
    idle_tick_mappings: Vec<usize>,
    // ---
    one_euro_filter_state: UncheckedRefCell<HashMap<ObjId, crate::filters::OneEuroFilter>>,
    raise_fall_state: UncheckedRefCell<HashMap<ObjId, RaiseFallMappingState>>,
    integrate_state: UncheckedRefCell<HashMap<ObjId, IntegrateMappingState>>,
    steering_state: UncheckedRefCell<HashMap<ObjId, SteeringMappingState>>,
    ema_state: UncheckedRefCell<HashMap<ObjId, crate::filters::EmaFilter>>,
    script_state: UncheckedRefCell<HashMap<ObjId, ScriptMappingState>>,
}

impl<'driver_loop> MappingEngine<'driver_loop> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        debug: DebugLevel,
        debug_idle_tick: bool,
        cfg: Config,
        hid_mgr: &'driver_loop HidManager,
        #[cfg(feature = "midi")] midi_mgr: MidiManager,
        shared_atomic_state: &'driver_loop SharedAtomicState,
    ) -> Result<Self> {
        Ok(Self {
            // ---
            running: false,
            idle_tick_rate: cfg.global.idle_tick_rate,
            // ---
            debug,
            debug_idle_tick,
            // ---
            cfg,
            hid_mgr,
            #[cfg(feature = "midi")]
            midi_mgr,
            // ---
            router_index_sysdev_and_ctl_type_to_cms_and_mappings: Default::default(),
            router_buff_mappings_to_execute: Default::default(),
            info_sysdev_to_enabled_mappings: Default::default(),
            // ---
            idle_tick_mappings: Default::default(),
            // ---
            integrate_state: Default::default(),
            steering_state: Default::default(),
            raise_fall_state: Default::default(),
            one_euro_filter_state: Default::default(),
            ema_state: Default::default(),
            _shared_atomic_state: shared_atomic_state,
            script_state: Default::default(),
        })
    }

    pub(crate) fn set_cfg(&mut self, cfg: Config) {
        self.cfg = cfg;
        self.scripting_cache_reset();
    }

    pub(crate) fn set_mappings(&mut self, mappings: &[Mapping]) {
        self.cfg.mappings = mappings.to_vec();
        self.scripting_cache_reset();
    }

    pub(crate) fn get_idle_tick_rate(&self) -> u32 {
        self.idle_tick_rate
    }

    pub(crate) fn set_idle_tick_rate(&mut self, rate: u32) {
        self.idle_tick_rate = rate.clamp(crate::config::MIN_BASE_FREQ_HZ, crate::config::MAX_BASE_FREQ_HZ);
        log::info!("Set base (idle tick) update rate to {}", self.idle_tick_rate);
    }

    pub(crate) fn active_mappings_count(&self) -> usize {
        self.cfg.mappings.iter().filter(|m| m.enabled).count()
    }

    // In order to provide shorter names for variables, the following acronims are used:
    // sysdev: system device: a device available in current system including virtual ones.
    // dmk: Device matcher key (a config key with which we reference a device mather, used as 'device: "mydevice"').
    // dm: Device matcher.
    // cmk: Control matcher key.
    // cm: Control matcher.
    pub(crate) fn init_mapping_router(&mut self) -> Result<()> {
        info!("Initializing mapping engine router.");

        self.idle_tick_mappings_reset();

        // ---
        self.router_index_sysdev_and_ctl_type_to_cms_and_mappings.clear();
        self.router_buff_mappings_to_execute.clear();
        // ---
        self.info_sysdev_to_enabled_mappings.clear();

        // +++++++++++++++++++++++++++++++++++++++++++++++++++++++++
        let available_hid_devices = self.hid_mgr.enumerate_available_devices(Some(
            HidDeviceKind::Mouse | HidDeviceKind::Keyboard | HidDeviceKind::Gamepad | HidDeviceKind::Joystick,
        ));

        let mut collect_enabled_mappings_for_dmk_and_cm =
            |dmk: &str, cm_id: ObjId, mappings: &mut Vec<_>, opened_device_id: ObjId| {
                for (mapping_idx, mapping) in self.cfg.mappings.iter().enumerate().filter(|(_, m)| m.enabled) {
                    for source in collect_dynamic_value_matchers(mapping, |ctx| {
                        ctx.contains(DynValFilter::Control | DynValFilter::Src)
                    })
                    .iter()
                    .map(|v| match v {
                        DynValueRefs::DeviceControlMatcher(dcm) => dcm.clone(),
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>()
                    {
                        if source.device_matcher_key == *dmk && source.control_matcher.get_id() == cm_id {
                            mappings.push(mapping_idx);
                            mappings.sort();
                            mappings.dedup();

                            self.info_sysdev_to_enabled_mappings
                                .entry(opened_device_id)
                                .or_default()
                                .push(mapping_idx);
                        }
                    }
                }
            };

        for available_hid_device_info in &available_hid_devices {
            for (dmk, dm) in self
                .cfg
                .devices
                .hid
                .iter()
                .filter(|(_, v)| {
                    v.is_enabled()
                        && v.matcher_name_regex_ref()
                            .and_then(|r| Some(r.is_match(&available_hid_device_info.name)))
                            .or(v
                                .virtual_device_name_ref()
                                .and_then(|n| Some(n == available_hid_device_info.name)))
                            .unwrap_or_default()
                })
                .collect::<Vec<(_, _)>>()
            {
                let opened_device_info = self.hid_mgr.open(available_hid_device_info.clone(), dmk, dm)?;
                let opened_device_id = opened_device_info.id;

                for (_, cm) in &dm.controls {
                    let (cms, mappings) = self
                        .router_index_sysdev_and_ctl_type_to_cms_and_mappings
                        .entry((opened_device_id, cm.r#type))
                        .or_default();
                    cms.push(ControlMatchers::Hid(cm.clone()));
                    collect_enabled_mappings_for_dmk_and_cm(dmk, cm.get_id(), mappings, opened_device_id);
                }
            }
        }

        #[cfg(feature = "midi")]
        let available_midi_devices = self.midi_mgr.enumerate_available_devices();
        #[cfg(feature = "midi")]
        for available_midi_device_info in &available_midi_devices {
            for (dmk, dm) in self
                .cfg
                .devices
                .midi
                .iter()
                .filter(|(_, v)| v.enabled && v.match_name_regex.is_match(&available_midi_device_info.name))
                .collect::<Vec<(_, _)>>()
            {
                let opened_device_id = self.midi_mgr.open(&available_midi_device_info.name)?;

                for (_, cm) in &dm.controls {
                    let (cms, mappings) = self
                        .router_index_sysdev_and_ctl_type_to_cms_and_mappings
                        .entry((opened_device_id, cm.midi_message.r#type.into()))
                        .or_default();
                    cms.push(ControlMatchers::Midi(cm.clone()));
                    collect_enabled_mappings_for_dmk_and_cm(dmk, cm.get_id(), mappings, opened_device_id);
                }
            }
        }

        for (_, v) in self.info_sysdev_to_enabled_mappings.iter_mut() {
            v.sort();
            v.dedup();
        }

        // ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info!(
            "Mapping Router built. Active Source Devices: {}",
            self.info_sysdev_to_enabled_mappings.len()
        );

        if self.debug.is_on() {
            let _ = fs::write(
                "MMVJ.mapping_router_debug.txt",
                format!("{:#?}", self.router_index_sysdev_and_ctl_type_to_cms_and_mappings),
            );
        }

        Ok(())
    }

    pub(crate) fn idle_tick_mappings_reset(&mut self) {
        self.idle_tick_mappings.clear();
        self.cfg
            .mappings
            .iter()
            .enumerate()
            .filter(|(_, mapping)| mapping.enabled && mapping.requires_idle_tick)
            .for_each(|(idx, _)| self.idle_tick_mappings.push(idx));
    }

    pub(crate) fn scripting_cache_reset(&mut self) {
        self.script_state.borrow_mut().clear();
    }

    pub(crate) fn _filters_state_reset(&mut self) {
        self.one_euro_filter_state.borrow_mut().clear();
        self.ema_state.borrow_mut().clear();
        self.integrate_state.borrow_mut().clear();
        self.raise_fall_state.borrow_mut().clear();
        self.steering_state.borrow_mut().clear();
    }

    pub(crate) async fn run(&mut self) {
        self.running = true;
        let mut ticker = interval(Duration::from_secs_f64(1.0 / self.idle_tick_rate as f64));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        //-------------------------------- MAIN LOOP ----------------------------------
        const DEBUG_MAIN_LOOP_LATENCY: bool = false; // TODO: generalized stats data, observable via Gui.
        while self.running {
            let main_loop_iter_start = std::time::Instant::now();

            #[cfg(feature = "midi")]
            select! {
            Some(midi_msg) = self.midi_mgr.consume_any_opened_device_message()=> self.map_midi_message(midi_msg),
            Some(hid_event) =  self.hid_mgr.consume_any_opened_device_event() => self.map_hid_event(hid_event),
            _ = ticker.tick() => self.process_idle_tick() }

            #[cfg(not(feature = "midi"))]
            select! {
            Some(hid_event) =  self.hid_mgr.consume_any_opened_device_event() => self.map_hid_event(hid_event),
            _ = ticker.tick() => self.process_idle_tick()}

            if DEBUG_MAIN_LOOP_LATENCY {
                dbg!((std::time::Instant::now() - main_loop_iter_start).as_millis());
            }
        }
    }

    pub(crate) fn stop(&mut self) -> Result<()> {
        self.running = false;

        // NB: joysticks are stopped/started/restarted externally to mapping engine
        // NB: to support persistence.
        #[cfg(feature = "midi")]
        let midi_stop_result = self.midi_mgr.stop().context("Failed to stop Midi Manager.");

        let mouse_stop_result = self.hid_mgr.stop(false).context("Failed to stop HID Manager.");

        let errors: Vec<String> = [
            #[cfg(feature = "midi")]
            midi_stop_result,
            mouse_stop_result,
        ]
        .into_iter()
        .filter_map(|res| res.err().map(|e| format!("- {}", e)))
        .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            let error_message = format!("One or more managers failed to stop:\n{}", errors.join("\n"));
            bail!(error_message)
        }
    }

    fn set_idle_tick_enabled_on_device_control_for_mapping(&self, mapping: &Mapping) {
        if let Some(flag) = mapping.dst.get_idle_tick_enabled_flag()
            && let Ok(prev) = flag.compare_exchange(
                !mapping.requires_idle_tick,
                mapping.requires_idle_tick,
                // Yes, both relaxed because active mapping will retrigger this, no need for stronger.
                Relaxed,
                Relaxed,
            )
            && self.debug.is_on()
        {
            debug!(
                "Idle-tick update for {}: {} -> {}",
                mapping.dst, prev, mapping.requires_idle_tick
            );
        }
    }

    #[cfg(feature = "midi")]
    fn map_midi_message(&mut self, msg: MappedMidiMessage) {
        if let Some((cms, mappings)) = self
            .router_index_sysdev_and_ctl_type_to_cms_and_mappings
            .get(&(msg.device_id, msg.message_type.into()))
        {
            cms.iter()
                .filter(|cm| {
                    let ControlMatchers::Midi(cm) = cm else { unreachable!() };
                    msg.matches_control_matcher(cm)
                })
                .for_each(|cm| cm.set_last_known_io(msg.get_value()));
            self.router_buff_mappings_to_execute.extend(mappings);
            self.run_mappings__(msg.device_id);
        }
    }

    fn map_hid_event(
        &mut self,
        event: MappedDeviceEvent, /* TODO: API! provide device_id within MappedHidEvent and simplify */
    ) {
        if let MappedDeviceEvent {
            device_id,
            event: MappedEvents::Hid(MappedHidEvent { control_type, value }),
        } = event
            && let Some((cms, mappings)) = self
                .router_index_sysdev_and_ctl_type_to_cms_and_mappings
                .get(&(device_id, control_type))
        {
            cms.iter().for_each(|cm| cm.set_last_known_io(value));
            self.router_buff_mappings_to_execute.extend(mappings);
            self.run_mappings__(device_id);
        }
    }

    fn run_mappings__(&mut self, triggering_device_id: ObjId) {
        self.router_buff_mappings_to_execute.sort();
        self.router_buff_mappings_to_execute.dedup();
        for mapping_idx in &self.router_buff_mappings_to_execute {
            let mapping = &self.cfg.mappings[*mapping_idx];
            self.execute_mapping_on_active_input(triggering_device_id, mapping, mapping.src.get_last_known_io());
        }
        self.router_buff_mappings_to_execute.clear();
    }

    fn execute_mapping_on_active_input(
        &self,
        runtime_input_device_id: ObjId,
        mapping: &Mapping,
        input_value: BaseNumT,
    ) {
        mapping.last_in.store(input_value as BaseNumT, Relaxed);
        if let ValueSrcs::Dynamic(DynValueRefs::DeviceControlMatcher(d)) = &mapping.src {
            d.control_matcher.set_last_known_io(input_value)
        }

        let final_value = self.apply_transformation_for_mapping(runtime_input_device_id, mapping, input_value, false);

        mapping.last_out.store(final_value as BaseNumT, Relaxed);

        match &mapping.dst {
            ValueDsts::Void => {}
            ValueDsts::Dynamic(d) => {
                self.set_dyn_value(d, final_value, self.debug);
                self.set_idle_tick_enabled_on_device_control_for_mapping(mapping);
            }
        }

        if self.debug.is_on() {
            debug!(
                "Mapped {} ({}): {} -> {}",
                mapping.name, mapping, input_value, final_value
            );
        }
    }

    fn process_idle_tick(&self) {
        for idx in &self.idle_tick_mappings {
            let mapping = &self.cfg.mappings[*idx];
            if let Some(flag) = mapping.dst.get_idle_tick_enabled_flag()
                && !flag.load(Relaxed)
            {
                continue;
            }

            let idle_in_value = self.get_value_src(mapping.src.get_interval(), &mapping.src, true);

            mapping.last_in.store(idle_in_value as BaseNumT, Relaxed);

            let final_value =
                self.apply_transformation_for_mapping(ObjId::from(usize::MAX), mapping, idle_in_value, true);

            mapping.last_out.store(final_value as BaseNumT, Relaxed);

            match &mapping.dst {
                ValueDsts::Void => {}
                ValueDsts::Dynamic(d) => {
                    self.set_dyn_value(d, final_value, self.debug_idle_tick.into());
                }
            }
        }
    }

    #[cfg(feature = "gui")]
    fn gui_trace_transform_step(
        &self,
        store_last_in_out: bool, // TODO: perf: always on.
        stage: gui_transform_step::TfmStepTraceStage,
        step_ref: &TfmStepCfg,
        vd: &MappedValue<BaseNumT>,
    ) {
        if store_last_in_out {
            match stage {
                TfmStepTraceStage::In => step_ref.get_state().last_in.store(vd.value as f32, Relaxed),
                TfmStepTraceStage::Out => step_ref.get_state().last_out.store(vd.value as f32, Relaxed),
                _ => {}
            }
        }

        step_ref.get_state().gui_trace(stage, vd, Instant::now());
    }

    fn apply_transformation_for_mapping(
        &self,
        runtime_input_device_id: ObjId,
        mapping: &'driver_loop Mapping,
        value: BaseNumT,
        is_idle_tick: bool,
    ) -> BaseNumT {
        let (src_interval, src_relativity) = (mapping.src.get_interval(), mapping.src.get_relativity());

        let src_interval = src_interval.cast::<BaseNumT>().expect(
            "Failed to cast source contol interval to BaseNumericT-based interval\
                which should not happen unless error in implementation.",
        );

        let dst_interval = mapping.dst.get_interval();

        self.apply_transformation(
            mapping,
            &mapping.transformation,
            runtime_input_device_id,
            src_interval,
            Some(dst_interval),
            value,
            src_relativity,
            is_idle_tick,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_transformation(
        &self,
        mapping: &'driver_loop Mapping,
        transfomation: &TfmSeqCfg,
        runtime_input_device_id: ObjId,
        src_interval: NumInterval<BaseNumT>,
        dst_interval: Option<NumInterval<BaseNumT>>,
        value: BaseNumT,
        is_relative: Relativity,
        is_idle_tick: bool,
    ) -> BaseNumT {
        let mut vd = MappedValue::<BaseNumT> {
            value,
            interval: src_interval,
            relativity: is_relative,
        };

        if !vd.interval.contains_inclusive(vd.value) {
            warn!(
                "The value (={}) read from device {} \
                        is out of configured interval ({:?}), clamping it.",
                vd.value,
                if is_idle_tick {
                    "idle tick"
                } else {
                    get_interned_str(*runtime_input_device_id).unwrap_or_default()
                },
                vd.interval
            );
            vd.value = vd.interval.clamp(vd.value);
        }

        for step in transfomation.steps.iter() {
            vd = self.apply_transformation_step(mapping, step, vd, is_idle_tick);
        }

        if let Some(dst_interval) = dst_interval
            && vd.interval != dst_interval
        {
            vd.value = dst_interval.map_from(vd.value, &vd.interval, OutOfRangePolicy::WarnAndClamp);
        }

        vd.value
    }

    fn apply_script(
        &self,
        step_id: ObjId,
        script_cfg: &ScriptCfg,
        mapping: &Mapping,
        is_idle_tick: bool,
        mut vd: MappedValue<BaseNumT>,
    ) -> MappedValue<BaseNumT> {
        match script_cfg.lang {
            crate::schemas_transform::ScriptLanguage::Luau => {
                self.script_state.borrow_mut().entry(step_id).or_insert_with(|| {
                    if self.debug.is_on() {
                        log::debug!("Compiling Luau script referenced in mapping {}", &mapping.name);
                    }
                    let lua = Lua::new();
                    let inputs = lua.create_table().unwrap();
                    let outputs = lua.create_table().unwrap();
                    let aux_tfm_idx = lua.create_table().unwrap();
                    let compiled = lua
                        .load(&script_cfg.script)
                        .into_function()
                        .inspect_err(|e| log::error!("{e}"))
                        .unwrap_or(lua.load(" ").into_function().unwrap());
                    let mapping_engine_ptr = self as *const Self as *const ();
                    let mapping_ptr = mapping as *const _ as *const ();
                    let tfms_ptr = &script_cfg.aux_transformations as *const _ as *const ();
                    let run_tfm_func = lua
                        .create_function(
                            move |_, args: (usize, BaseNumT)| -> std::result::Result<BaseNumT, mlua::Error> {
                                let tfm_idx = args.0;
                                let input_value = args.1;
                                // SAFETY: scripting cache MUST be reset (scripting_cache_reset())
                                // whenever configuration is updated beyond trivial changes like parameter values changes.
                                let mapping_engine: &Self = unsafe { &*(mapping_engine_ptr as *const Self) };
                                let mapping: &Mapping = unsafe { &*(mapping_ptr as *const Mapping) };
                                let tfms = unsafe { &*(tfms_ptr as *const BTreeMap<String, TfmSeqCfg>) };
                                if tfm_idx < tfms.len() {
                                    let tfm = tfms.values().nth(tfm_idx).unwrap();
                                    Ok(mapping_engine.apply_transformation(
                                        mapping,
                                        tfm,
                                        // ObjId::from(INTERNER.get_or_intern(&mapping.name).into_usize()), // TODO: perf?
                                        ObjId::from(usize::MAX),
                                        tfm.get_interval(),
                                        None,
                                        input_value,
                                        tfm.get_relativity(),
                                        is_idle_tick,
                                    ))
                                } else {
                                    Err(mlua::Error::RuntimeError(format!(
                                        "Referenced transformation {} is not found. \
                                Total transformations available for the script: {}, indexing starting from 0 ",
                                        tfm_idx,
                                        tfms.len()
                                    )))
                                }
                            },
                        )
                        .unwrap();
                    let _ = lua
                        .globals()
                        .set("transform", run_tfm_func)
                        .inspect_err(|e| log::error!("{e}"));
                    let _ = lua
                        .globals()
                        .set("inputs", inputs.clone())
                        .inspect_err(|e| log::error!("{e}"));
                    let _ = lua
                        .globals()
                        .set("outputs", outputs.clone())
                        .inspect_err(|e| log::error!("{e}"));

                    let _ = inputs.set("idle_tick_rate", self.get_idle_tick_rate());

                    for (idx, (name, _)) in script_cfg.aux_transformations.iter().enumerate() {
                        // let _ = lua.globals().set(name.as_str(), idx);
                        let _ = aux_tfm_idx.set(name.as_str(), idx);
                    }
                    let _ = lua
                        .globals()
                        .set("aux_tfm_idx", aux_tfm_idx)
                        .inspect_err(|e| log::error!("{e}"));

                    ScriptMappingState {
                        lua,
                        inputs,
                        outputs,
                        compiled,
                    }
                });

                let (inputs, outputs, compiled) = {
                    let data = self.script_state.borrow();
                    let script_data = data.get(&step_id).unwrap();
                    (
                        script_data.inputs.clone(),
                        script_data.outputs.clone(),
                        script_data.compiled.clone(),
                    )
                };

                // -----------------------------------
                // Set runtime inputs.
                // -----------------------------------
                let _ = inputs.set("is_idle_tick", is_idle_tick);
                let _ = inputs.set(0, vd.value);
                for (idx, (name, src)) in script_cfg.aux_srcs.iter().enumerate() {
                    let input_val = self.get_value_src(
                        src.remap_to_interval.unwrap_or(src.source.get_interval()),
                        &src.source,
                        is_idle_tick,
                    );

                    let _ = inputs.set(idx + 1, input_val).inspect_err(|e| log::error!("{e}"));
                    let _ = inputs.set(name.as_str(), input_val).inspect_err(|e| log::error!("{e}"));
                    //let _ = lua.globals().set(name.as_str(), input_val).inspect_err(|e| log::error!("{e}"));
                }

                if let Err(e) = compiled.call::<()>(()) {
                    log::error!("{e} ");
                } else {
                    // -----------------------------------
                    // Set outputs.
                    // -----------------------------------
                    vd.value = outputs.get(0).unwrap_or(vd.value);
                    for (idx, (name, dst)) in script_cfg.aux_dsts.iter().enumerate() {
                        match &dst.destination {
                            ValueDsts::Void => {}
                            ValueDsts::Dynamic(dynamic_value_refs_rt) => {
                                if let Ok(mut out) = outputs.get(name.as_str()).or(outputs.get(idx + 1)) {
                                    if let Some(from_interval) = dst.remap_from_interval {
                                        out = dst.destination.get_interval().map_from(
                                            out,
                                            &from_interval,
                                            OutOfRangePolicy::WarnAndClamp,
                                        );
                                    }
                                    self.set_dyn_value(dynamic_value_refs_rt, out, self.debug);
                                }
                            }
                        }
                    }
                }

                // let _ = inputs.clear();
                let _ = outputs.clear();

                // -----------------------------------
                vd.relativity = script_cfg.output_relativity.unwrap_or(vd.relativity);
            }
        }

        vd
    }

    fn apply_transformation_step(
        &self,
        mapping: &'driver_loop Mapping,
        step: &TfmStepCfg,
        mut vd: MappedValue<BaseNumT>,
        is_idle_tick: bool,
    ) -> MappedValue<BaseNumT> {
        #[cfg(feature = "gui")]
        self.gui_trace_transform_step(
            mapping.store_last_in_out, // TODO: perf: always on.
            gui_transform_step::TfmStepTraceStage::In,
            step,
            &vd,
        );

        match step {
            TfmStepCfg::Nop { .. } => {}
            TfmStepCfg::Script { script, .. } => {
                if script.enabled {
                    vd = self.apply_script(step.get_id(), script, mapping, is_idle_tick, vd);
                    vd.interval = step.get_state().get_out_interval();
                }
            }
            TfmStepCfg::Invert { invert, .. } => {
                if invert.enabled {
                    vd.value = self.apply_invert(vd.to_owned());
                }
            }
            TfmStepCfg::Integrate { integrate, .. } => {
                if integrate.enabled {
                    // assert!(
                    //     vd.is_relative == Relativity::Rel,
                    //     "Integrate transform must only be applied to relative inputs."
                    // );
                    if !is_idle_tick || integrate.on_idle {
                        vd = self.apply_integrate(step.get_id(), integrate, vd.value)
                    }
                }
            }
            TfmStepCfg::Clamp { clamp, state } => {
                if clamp.enabled {
                    let in_interval = state.0.read().get_in_interval();
                    vd.value = clamp.get_clamping_interval(in_interval).clamp(vd.value);
                    vd.interval = clamp.get_out_interval(in_interval);
                    // Clamping interval may not contain current value, for now this is not an error.
                    // In such a case the above clamping is a nop.
                    // If we override out interval with clamping interval the value will not fit,
                    // so we need to clamp value to the out interval also.
                    vd.value = vd.interval.clamp(vd.value);
                }
            }
            TfmStepCfg::Steering { steering, .. } => {
                if steering.enabled {
                    // if vd.relativity.into() {
                    //     warn!("Steering transform should only be applied to relative inputs.");
                    // }
                    vd = self.apply_steering(mapping, step, steering, vd, Instant::now(), is_idle_tick);
                }
            }
            TfmStepCfg::RaiseFall { raise_fall, .. } => {
                if raise_fall.enabled {
                    if vd.relativity != Relativity::Abs {
                        log::warn!("Raise-fall transform should only be applied to absolute inputs.");
                    }
                    vd = self.apply_raise_fall(step.get_id(), raise_fall, vd, is_idle_tick);
                }
            }
            TfmStepCfg::Ema { ema: ema_filter, .. } => {
                if ema_filter.enabled
                    && (!is_idle_tick || vd.relativity == Relativity::Abs || ema_filter.on_relative_input_feed_on_idle)
                {
                    vd.value = self.apply_ema_filter(step.get_id(), ema_filter, vd.value)
                } else if ema_filter.on_relative_input_reset_on_idle {
                    self.reset_ema_filter(step.get_id(), vd.value);
                }
            }
            TfmStepCfg::Linear { linear, .. } => {
                if linear.enabled && (!is_idle_tick || linear.on_idle) {
                    vd.value = self.apply_linear(linear, vd.value, vd.interval);
                }
            }
            TfmStepCfg::Smoothstep {
                smoothstep: smoothstep_curve,
                ..
            } => {
                if smoothstep_curve.enabled && (!is_idle_tick || smoothstep_curve.on_idle) {
                    vd.value = self.apply_smoothstep(vd.value, vd.interval);
                }
            }
            TfmStepCfg::SCurve { s_curve, .. } => {
                if s_curve.enabled && (!is_idle_tick || s_curve.on_idle) {
                    vd.value = self.apply_s_curve(s_curve, vd.value, vd.interval);
                }
            }
            TfmStepCfg::NormExp { exp, state: _ } => {
                if exp.enabled && (!is_idle_tick || exp.on_idle) {
                    vd.value = self.apply_norm_exp_curve(exp, vd.value, vd.interval);
                }
            }
            TfmStepCfg::SignedPower {
                signed_power: signed_power_curve,
                ..
            } => {
                if signed_power_curve.enabled && (!is_idle_tick || signed_power_curve.on_idle) {
                    vd.value = self.apply_signed_power_curve(signed_power_curve, vd.value, vd.interval);
                }
            }
            TfmStepCfg::OneEuro { one_euro, .. } => {
                if one_euro.enabled
                    && (!is_idle_tick || vd.relativity == Relativity::Abs || one_euro.on_relative_input_feed_on_idle)
                {
                    vd.value = self.apply_one_euro_filter(step.get_id(), one_euro, vd.value);
                } else if one_euro.on_relative_input_reset_on_idle {
                    self.reset_one_euro_filter(step.get_id(), vd.value);
                }
            }
            TfmStepCfg::_HighPass { highpass, .. } => {
                if highpass.enabled {
                    let __v = // Update state.
                        self.apply_high_pass_filter(step, highpass, vd.value);
                    if !is_idle_tick || highpass.on_idle {
                        vd.value = __v;
                    }
                }
            }
            TfmStepCfg::_ForceFeedback {
                state: _,
                force_feedback: _,
            } => {
                todo!(
                    "Standalone force feedback transform is WIP   . Currently supported only within steering transform."
                )
            }
        }

        if !vd.interval.contains_inclusive(vd.value) {
            log::warn!(
                "Value {} must fit in interval {} after transformation step ``{}'' (ID: {}). 
            Each step must ensure it, clamping!",
                vd.value,
                vd.interval,
                step,
                step.get_id()
            );
            vd.value = vd.interval.clamp(vd.value);
        }

        #[cfg(feature = "gui")]
        self.gui_trace_transform_step(
            mapping.store_last_in_out, // TODO: perf: always on.
            gui_transform_step::TfmStepTraceStage::Out,
            step,
            &vd,
        );

        vd
    }

    fn get_ema_filter(&self, state_id: ObjId, value: BaseNumT) -> UncheckedRefMut<'_, crate::filters::EmaFilter> {
        UncheckedRefMut::map(self.ema_state.borrow_mut(), |map| {
            map.entry(state_id)
                .or_insert_with(|| crate::filters::EmaFilter::new(value, Instant::now()))
        })
    }

    fn apply_ema_filter(&self, state_id: ObjId, ema: &EmaFilterCfg, value: BaseNumT) -> BaseNumT {
        self.get_ema_filter(state_id, value)
            .filter(value, Instant::now(), ema.tau)
    }

    fn reset_ema_filter(&self, state_id: ObjId, value: BaseNumT) {
        self.get_ema_filter(state_id, value).reset(value)
    }

    fn get_one_euro_filter(&self, state_id: ObjId, value: BaseNumT) -> UncheckedRefMut<'_, OneEuroFilter> {
        UncheckedRefMut::map(self.one_euro_filter_state.borrow_mut(), |map| {
            map.entry(state_id)
                .or_insert(crate::filters::OneEuroFilter::new(value, Instant::now()))
        })
    }

    fn apply_one_euro_filter(&self, state_id: ObjId, one_euro: &OneEuroFilterCfg, value: BaseNumT) -> BaseNumT {
        self.get_one_euro_filter(state_id, value).filter(
            value,
            Instant::now(),
            one_euro.min_cutoff_hz,
            one_euro.beta,
            one_euro.d_cutoff_hz,
        )
    }

    fn reset_one_euro_filter(&self, state_id: ObjId, value: BaseNumT) {
        self.get_one_euro_filter(state_id, value).reset(value)
    }

    fn apply_invert(&self, vd: MappedValue<BaseNumT>) -> BaseNumT {
        match vd.relativity {
            Relativity::Rel => -vd.value,
            Relativity::Abs => vd.interval.clamp_and_invert(vd.value), // TODO?: just invert, no clamp here needed.
        }
    }

    fn apply_linear(&self, linear: &LinearCfg, value: BaseNumT, interval: NumInterval<BaseNumT>) -> BaseNumT {
        if linear.center_symmetric {
            Curves::apply_center_symmetric_with_abs_value(
                value,
                interval,
                |abs_v| {
                    Curves::linear(
                        abs_v,
                        linear.slope,
                        interval.map_to_symm_unit(linear.shift_x, OutOfRangePolicy::Clamp),
                        interval.map_to_symm_unit(linear.shift_y, OutOfRangePolicy::Clamp),
                    )
                },
                OutOfRangePolicy::Clamp,
            )
        } else {
            interval.clamp(Curves::linear(value, linear.slope, linear.shift_x, linear.shift_y))
        }
    }

    fn apply_smoothstep(&self, value: BaseNumT, interval: NumInterval<BaseNumT>) -> BaseNumT {
        interval.map_from_unit(
            Curves::smoothstep(interval.map_to_unit(value, OutOfRangePolicy::WarnAndClamp)),
            OutOfRangePolicy::WarnAndClamp,
        )
    }

    fn apply_s_curve(&self, s_curve: &SCurveCfg, value: BaseNumT, interval: NumInterval<BaseNumT>) -> BaseNumT {
        interval.map_from_unit(
            Curves::s_curve(
                interval.map_to_unit(value, OutOfRangePolicy::WarnAndClamp),
                s_curve.steepness,
            ),
            OutOfRangePolicy::WarnAndClamp,
        )
    }

    fn apply_norm_exp_curve(&self, exp: &NormExpCfg, value: BaseNumT, interval: NumInterval<BaseNumT>) -> BaseNumT {
        if exp.center_symmetric {
            Curves::apply_center_symmetric_with_abs_value(
                value,
                interval,
                |v_abs| Curves::exp_curve(v_abs, exp.base),
                OutOfRangePolicy::WarnAndClamp,
            )
        } else {
            interval.map_from_unit(
                Curves::exp_curve(interval.map_to_unit(value, OutOfRangePolicy::WarnAndClamp), exp.base),
                OutOfRangePolicy::WarnAndClamp,
            )
        }
    }

    fn apply_signed_power_curve(
        &self,
        power: &SignedPowerCfg,
        value: BaseNumT,
        interval: NumInterval<BaseNumT>,
    ) -> BaseNumT {
        if power.center_symmetric {
            Curves::apply_center_symmetric_with_abs_value(
                value,
                interval,
                |v_abs| Curves::signed_power(v_abs, power.power),
                OutOfRangePolicy::WarnAndClamp,
            )
        } else {
            interval.map_from_unit(
                Curves::signed_power(interval.map_to_unit(value, OutOfRangePolicy::WarnAndClamp), power.power),
                OutOfRangePolicy::WarnAndClamp,
            )
        }
    }

    fn apply_high_pass_filter(&self, _step: &TfmStepCfg, _high_pass: &HighPassCfg, _value: BaseNumT) -> BaseNumT {
        todo!("Highpass filter is WIP.")
    }

    fn apply_integrate(&self, state_id: ObjId, integrate: &IntegrateCfg, mut delta: BaseNumT) -> MappedValue<BaseNumT> {
        // TODO?: if non-relative, consider automatically differentiate before application, maybe under an option:
        //        Abs data can always be > 0 so it can just make outptu stick to max value.

        let mut data = self.integrate_state.borrow_mut();

        if delta.abs() < integrate.deadzone_norm * integrate.range.span() {
            delta = 0.0;
        }

        let state = data.entry(state_id).or_insert(IntegrateMappingState {
            prev_val: (integrate.range.from() + integrate.range.to()) * 0.5,
        });

        let new_val = state.prev_val + delta;
        let new_val_smoothed = new_val * integrate.smoothing_alpha + state.prev_val * (1.0 - integrate.smoothing_alpha);

        state.prev_val = integrate.range.clamp(new_val_smoothed);

        MappedValue::<BaseNumT> {
            value: state.prev_val,
            interval: integrate.range,
            relativity: Relativity::Abs,
        }
    }

    fn apply_steering(
        &self,
        mapping: &'driver_loop Mapping,
        step: &TfmStepCfg,
        steering: &SteeringCfg,
        vd: MappedValue<BaseNumT>,
        now: Instant,
        is_idle_tick: bool,
    ) -> MappedValue<BaseNumT> {
        let mut state = *self
            .steering_state
            .borrow_mut()
            .entry(step.get_id())
            .or_insert(SteeringMappingState {
                last_time: now,
                pre_filter: 0.0,
                post_filter: 0.0,
            });

        let value = vd.value * self.get_value_src(UNIT_INTERVAL, &steering.input_gain, is_idle_tick);

        let auto_center_along_force_feedback =
            self.get_value_src(UNIT_INTERVAL, &steering.auto_center_along_force_feedback, is_idle_tick);

        let dt = (now - state.last_time).as_secs_f32() as BaseNumT;
        let delta: BaseNumT = vd.interval.map_to_symm_unit(value, OutOfRangePolicy::Clamp);

        if let Some(acc) = &steering.accumulator {
            state.pre_filter = self.get_dyn_value(SYMM_UNIT_INTERVAL, acc, is_idle_tick);
        }

        state.pre_filter = SYMM_UNIT_INTERVAL.clamp(state.pre_filter.add(delta));

        #[cfg(feature = "gui")]
        if delta != 0.0 {
            step.get_state().gui_trace(
                TfmStepTraceStage::Custom(
                    GraphDisplayStyle::as_filled()
                        .with_color(Color32::BROWN.gamma_multiply(0.7))
                        .with_width(1.2),
                ),
                &MappedValue::<BaseNumT> {
                    value: delta,
                    interval: SYMM_UNIT_INTERVAL,
                    relativity: Relativity::Rel,
                },
                now,
            );
        }

        #[cfg(feature = "gui")]
        step.get_state().gui_trace(
            TfmStepTraceStage::Custom(GraphDisplayStyle::as_filled().with_color(Color32::BLUE).with_width(1.5)),
            &MappedValue::<BaseNumT> {
                value: state.pre_filter,
                interval: SYMM_UNIT_INTERVAL,
                relativity: Relativity::Abs,
            },
            now,
        );

        '_User_input_filtering_and_curving_pre_FFB_and_autocentering: {
            if !steering.integrated_user_input_transform.steps.is_empty() {
                state.post_filter = self.apply_transformation(
                    mapping,
                    &steering.integrated_user_input_transform,
                    ObjId::from(usize::MAX), // "[steering transform user input transform]",
                    SYMM_UNIT_INTERVAL,
                    Some(SYMM_UNIT_INTERVAL),
                    state.pre_filter,
                    Relativity::Abs, // Not relative, we have integrated it already.
                    is_idle_tick,
                );
            } else {
                state.post_filter = state.pre_filter;
            }
        }

        #[cfg(feature = "gui")]
        step.get_state().gui_trace(
            TfmStepTraceStage::Custom(
                GraphDisplayStyle::default()
                    .with_color(Color32::MAGENTA)
                    .with_width(1.2),
            ),
            &MappedValue::<BaseNumT> {
                value: state.post_filter,
                interval: SYMM_UNIT_INTERVAL,
                relativity: Relativity::Abs,
            },
            now,
        );

        let hold_factor_unit = self
            .get_value_src(UNIT_INTERVAL, &steering.hold_factor, is_idle_tick)
            .clamp(0.0, 1.0);

        '_FFB_and_autocentering: {
            // TODO: perf: profile.
            let ff_force_symm_norm = if let Some(ff_config) = &steering.force_feedback {
                if ff_config.enabled {
                    let raw_force = if let Some(custom_src) = &ff_config.custom_source {
                        self.get_value_src(SYMM_UNIT_INTERVAL, custom_src, is_idle_tick)
                    } else {
                        match &mapping.dst {
                            ValueDsts::Void => 0.0,
                            ValueDsts::Dynamic(dynamic_value_ref_rt) => match dynamic_value_ref_rt {
                                DynValueRefs::DeviceControlMatcher(d) => match ff_config.component {
                                    ForceFeedbackComponent::X => {
                                        self.hid_mgr.ff_set_x_axis_pos(
                                            &d.device_matcher_key,
                                            &d.control_key,
                                            mapping.dst.get_interval(),
                                        );
                                        self.hid_mgr.ff_get_x_sum_symm_norm(&d.device_matcher_key)
                                    }
                                    ForceFeedbackComponent::Y => {
                                        self.hid_mgr.ff_set_y_axis_pos(
                                            &d.device_matcher_key,
                                            &d.control_key,
                                            mapping.dst.get_interval(),
                                        );
                                        self.hid_mgr.ff_get_y_sum_symm_norm(&d.device_matcher_key)
                                    }
                                },
                                DynValueRefs::Variable(_) => 0.0,
                            },
                        }
                    };

                    let filtered_force = if !ff_config.transformation.steps.is_empty() {
                        self.apply_transformation(
                            mapping,
                            &ff_config.transformation,
                            ObjId::from(usize::MAX), /* TODO: ID NAMESPACES */
                            // "[steering transform ffb transform]",
                            SYMM_UNIT_INTERVAL,
                            Some(SYMM_UNIT_INTERVAL),
                            raw_force,
                            Relativity::Abs, // FFB is absolute.
                            is_idle_tick,
                        )
                    } else {
                        raw_force
                    };

                    let filtered_and_scaled_force = SYMM_UNIT_INTERVAL.clamp(filtered_force * ff_config.gain);

                    if ff_config.invert {
                        -filtered_and_scaled_force
                    } else {
                        filtered_and_scaled_force
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            if ff_force_symm_norm.abs() > 1e-4 {
                let ff_position_offset = ff_force_symm_norm * (1.0 - hold_factor_unit) * dt;
                state.post_filter += ff_position_offset;
                state.pre_filter += ff_position_offset;

                if self.debug.is_on() && self.debug_idle_tick && ff_force_symm_norm.abs() > 0.1 {
                    debug!(
                        "FF active: force={:.3} offset={:.3}",
                        ff_force_symm_norm, ff_position_offset
                    );
                }

                #[cfg(feature = "gui")]
                step.get_state().gui_trace(
                    TfmStepTraceStage::Custom(
                        GraphDisplayStyle::default()
                            .with_color(
                                Color32::GREEN.gamma_multiply((1.0 as BaseNumT - hold_factor_unit).max(0.4) as f32),
                            )
                            .with_width(1.7),
                    ),
                    &MappedValue::<BaseNumT> {
                        value: ff_force_symm_norm,
                        interval: SYMM_UNIT_INTERVAL,
                        relativity: Relativity::Abs,
                    },
                    now,
                );
            }

            let autocentering_halflife = self.get_value_src(
                steering.auto_center_halflife.get_interval(),
                &steering.auto_center_halflife,
                is_idle_tick,
            );

            let ffb_is_small = ff_force_symm_norm.abs() < 1e-4;

            if autocentering_halflife > 0.0
                && (auto_center_along_force_feedback > 0.0 || ffb_is_small)
                && delta.abs() < 1e-4
            {
                let mut centerwize_decay_factor =
                    (1.0 - (-dt / autocentering_halflife).exp2()) * (1.0 - hold_factor_unit);

                if !ffb_is_small {
                    centerwize_decay_factor *= auto_center_along_force_feedback;
                }

                state.post_filter -= state.post_filter * centerwize_decay_factor;
                state.pre_filter -= state.pre_filter * centerwize_decay_factor;
            }
        };

        state.pre_filter = SYMM_UNIT_INTERVAL.clamp(state.pre_filter);
        state.post_filter = SYMM_UNIT_INTERVAL.clamp(state.post_filter);

        let out = MappedValue::<BaseNumT> {
            value: state.post_filter,
            interval: SYMM_UNIT_INTERVAL,
            relativity: Relativity::Abs,
        };

        state.last_time = now;

        if let Some(acc) = &steering.accumulator {
            self.set_dyn_value(
                acc,
                acc.get_interval()
                    .map_from_symm_unit(state.pre_filter, OutOfRangePolicy::Clamp),
                self.debug,
            );
        }

        self.steering_state.borrow_mut().insert(step.get_id(), state);

        out
    }

    fn set_dyn_value(&self, d: &DynValueRefs, val: BaseNumT, debug: DebugLevel) {
        match d {
            DynValueRefs::DeviceControlMatcher(d) => {
                d.control_matcher.set_last_known_io(val);

                match d.control_matcher {
                    #[cfg(feature = "midi")]
                    ControlMatchers::Midi(_) => {
                        log::warn!("Only supporting variables and owned virtual joysticks as destinations.")
                    }
                    ControlMatchers::Hid(_) => {
                        self.hid_mgr
                            .set_control_value(&d.device_matcher_key, &d.control_key, val, !debug.is_on())
                    }
                }
            }
            DynValueRefs::Variable(v) => v.variable.value.store(v.variable.interval.clamp(val) as f32, Relaxed),
        }
    }

    fn get_dyn_value(
        &self,
        tgt_interval: NumInterval<BaseNumT>,
        val_ref: &DynValueRefs,
        is_idle_tick: bool,
    ) -> BaseNumT {
        if is_idle_tick && val_ref.get_relativity() == Relativity::Rel {
            0.0
        } else {
            tgt_interval.map_from(
                val_ref.get_numeric_value(),
                &val_ref.get_interval(),
                OutOfRangePolicy::WarnAndClamp,
            )
        }
    }

    fn get_value_src(&self, tgt_interval: NumInterval<BaseNumT>, val_ref: &ValueSrcs, is_idle_tick: bool) -> BaseNumT {
        if is_idle_tick && val_ref.get_relativity() == Relativity::Rel {
            0.0
        } else {
            tgt_interval.map_from(
                val_ref.get_numeric_value(),
                &val_ref.get_interval(),
                OutOfRangePolicy::WarnAndClamp,
            )
        }
    }

    fn apply_raise_fall(
        &self,
        state_id: ObjId,
        raise_fall: &RaiseFallCfg,
        mut vd: MappedValue<BaseNumT>,
        is_idle_tick: bool,
    ) -> MappedValue<BaseNumT> {
        let now = Instant::now();
        let mut data = self.raise_fall_state.borrow_mut();
        let filter_data = data.entry(state_id).or_insert(RaiseFallMappingState {
            prev_out: vd.interval.from(),
            last_target: vd.interval.from(),
            prev_out_time: Some(now),
            prev_user_input_time: Some(now),
        });

        let dt = if let Some(prev) = filter_data.prev_out_time {
            (now - prev).as_secs_f32()
        } else {
            0.0
        } as BaseNumT;

        let dt_user_input = if let Some(prev) = filter_data.prev_user_input_time {
            (now - prev).as_secs_f32()
        } else {
            0.0
        } as BaseNumT;

        filter_data.prev_out_time = Some(now);

        let target = if !is_idle_tick {
            filter_data.last_target = vd.value;
            vd.value
        } else {
            filter_data.last_target
        };

        let mut final_out = filter_data.prev_out;
        if is_idle_tick {
            if dt > 0.0 {
                let delta_v = target - filter_data.prev_out;
                let rate_limit = if delta_v > 0.0 {
                    raise_fall.raise_rate
                } else {
                    let mut fall_hold_factor =
                        self.get_value_src(UNIT_INTERVAL, &raise_fall.fall_hold_factor, is_idle_tick);

                    if raise_fall.invert_fall_hold_factor {
                        fall_hold_factor = UNIT_INTERVAL.clamp_and_invert(fall_hold_factor);
                    }

                    if raise_fall.fall_delay > 0.0 {
                        if raise_fall.fall_delay < dt_user_input {
                            raise_fall.fall_rate * (1.0 - fall_hold_factor)
                        } else {
                            0.0
                        }
                    } else {
                        raise_fall.fall_rate * (1.0 - fall_hold_factor)
                    }
                };
                let max_delta = rate_limit * dt;
                let actual_delta = delta_v.clamp(-max_delta, max_delta);
                final_out = filter_data.prev_out + actual_delta;
            }

            let smoothing_alpha = raise_fall.smoothing_alpha;
            final_out = (smoothing_alpha) * final_out + (1.0 - smoothing_alpha) * filter_data.prev_out;

            final_out = vd.interval.clamp(final_out);
            filter_data.prev_out = final_out;
        } else {
            filter_data.prev_user_input_time = Some(now);
        }

        vd.value = final_out;
        vd
    }
}
