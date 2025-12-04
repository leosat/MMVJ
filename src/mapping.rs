use crate::base_num::BaseNumT;
use crate::interner::{get_interned_str, intern_str};

use crate::debug::DebugLevel;
use crate::debug::get_debug_level;
use crate::hid_device::HidDeviceKind;
use crate::hid_manager::{HidManager, WithDeviceClassification};
use crate::mapped_controls::MappedCtls;
use crate::mapped_device::{MappedDeviceEvent, MappedDeviceManager, MappedEvents, MappedHidEvent};
#[cfg(feature = "midi")]
use crate::midi::{MappedMidiMessage, MidiManager};
use crate::num_interval::{NumInterval, OutOfRangePolicy};
use crate::schemas_cfg::Config;
use crate::schemas_common::{ObjId, WithRuntimeId};
use crate::schemas_control_matcher::ControlMatchers;

use crate::schemas_mapping::Mapping;
use crate::schemas_transform::{DynValFilter, collect_dynamic_value_matchers};
use crate::schemas_value::{
    DynValueRefs, ValueDsts, WithLastKnownIOSettable, WithNumInterval, WithNumericValueSettable,
};
use crate::schemas_value::{MappedValue, WithNumericValue};
use crate::schemas_value::{ValueSrcs, WithRelativity};

use crate::tfm_exec::{TfmExecCtx, WithTfmExec};
use anyhow::Result;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::Ordering::Relaxed;
use tokio::select;
use tokio::time::{Duration, MissedTickBehavior, interval};

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) enum MappingEngineCmd {
    _None,
    #[default]
    UpdateMappingRouterIdleTickOnly,
    UpdateMappingRouter,
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
    //  Mapping router algorithm index and runtime buffer.
    // ---
    #[allow(clippy::type_complexity)]
    router_index_sysdev_and_ctl_type_to_cms_and_mappings:
        HashMap<(ObjId, MappedCtls), (Vec<ControlMatchers>, Vec<Vec<usize>>)>,
    router_buff_mappings_to_execute: Vec<usize>,
    // ---
    info_sysdev_to_enabled_mappings: HashMap<ObjId, Vec<usize>>, // NB: this is only used in mappings init routine, but leaving here for potential future use in other places.
    // ---
    idle_tick_mappings: Vec<usize>,
    lua: mlua::Lua,
}

impl<'driver_loop> MappingEngine<'driver_loop> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        debug: DebugLevel,
        debug_idle_tick: bool,
        cfg: Config,
        hid_mgr: &'driver_loop HidManager,
        #[cfg(feature = "midi")] midi_mgr: MidiManager,
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
            lua: mlua::Lua::new(),
        })
    }

    pub(crate) fn set_cfg(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    pub(crate) fn set_mappings(&mut self, mappings: &[Mapping]) {
        self.cfg.mappings = mappings.to_vec();
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
    pub(crate) fn init(&mut self) -> Result<()> {
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
            |dmk: &str, cm_id: ObjId, cm_idx, mappings: &mut Vec<Vec<usize>>, opened_device_id: ObjId| {
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
                            if mappings.get(cm_idx).is_none() {
                                mappings.resize(cm_idx + 1, Vec::new());
                            };
                            let q: &mut Vec<usize> = mappings.get_mut(cm_idx).unwrap();
                            q.push(mapping_idx);
                            q.sort();
                            q.dedup();

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
                    // TODO: logic for matching available devices with device matchers
                    // TODO: will go to a reusable routine for reuse in other parts, e.g. in Gui.
                    v.is_enabled()
                        && v.matcher_name_regex_ref()
                            .map(|r| r.is_match(&available_hid_device_info.name))
                            .or(v
                                .virtual_device_name_ref()
                                .map(|n| crate::hid_device::sanitize_hid_name(n) == available_hid_device_info.name))
                            .unwrap_or_default()
                        && v.get_classification()
                            .intersects(available_hid_device_info.classification)
                })
                .collect::<Vec<(_, _)>>()
            {
                let opened_device_info = self.hid_mgr.open(available_hid_device_info.clone(), dmk, dm)?;
                let opened_device_id = opened_device_info.id;

                for cm in dm.controls.values() {
                    let (cms, mappings) = self
                        .router_index_sysdev_and_ctl_type_to_cms_and_mappings
                        .entry((opened_device_id, cm.r#type))
                        .or_default();
                    cms.push(ControlMatchers::Hid(cm.clone()));
                    collect_enabled_mappings_for_dmk_and_cm(
                        dmk,
                        cm.get_id(),
                        cms.len() - 1,
                        mappings,
                        opened_device_id,
                    );
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

                for cm in dm.controls.values() {
                    let (cms, mappings) = self
                        .router_index_sysdev_and_ctl_type_to_cms_and_mappings
                        .entry((opened_device_id, cm.midi_message.r#type.into()))
                        .or_default();
                    cms.push(ControlMatchers::Midi(cm.clone()));
                    collect_enabled_mappings_for_dmk_and_cm(
                        dmk,
                        cm.get_id(),
                        cms.len() - 1,
                        mappings,
                        opened_device_id,
                    );
                }
            }
        }

        for v in self.info_sysdev_to_enabled_mappings.values_mut() {
            v.sort();
            v.dedup();
        }

        // ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info!(
            "Mapping Router built. Mapped source devices: {}",
            self.info_sysdev_to_enabled_mappings.len()
        );

        if self.debug.is_on() {
            let _ = fs::write(
                format!("{}.mapping_router_debug.txt", crate::config::APP_NAME),
                format!("{:#?}", self.router_index_sysdev_and_ctl_type_to_cms_and_mappings),
            );
        }

        // Run all mappings once on init.
        self.router_buff_mappings_to_execute.extend(
            self.cfg
                .mappings
                .iter()
                .enumerate()
                .filter(|(_, m)| m.enabled)
                .map(|(i, _)| i)
                .collect::<Vec<_>>(),
        );
        self.run_mappings__(ObjId::from(intern_str("Init")));
        self.router_buff_mappings_to_execute.clear();

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
        #[cfg(feature = "midi")]
        self.midi_mgr.stop()?;
        Ok(())
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
                .enumerate()
                .filter(|(_, cm)| {
                    let ControlMatchers::Midi(cm) = cm else { unreachable!() };
                    msg.matches_control_matcher(cm)
                })
                .for_each(|(cm_idx, cm)| {
                    cm.set_numeric_value(msg.get_operational_value());
                    if !mappings.is_empty() {
                        self.router_buff_mappings_to_execute.extend(&mappings[cm_idx]);
                    }
                });

            let dedup = cms.len() > 1;
            let shuffle = self.router_buff_mappings_to_execute.len() > 1;
            Self::dedup_and_shuffle(&mut self.router_buff_mappings_to_execute, dedup, shuffle);
            self.run_mappings__(msg.device_id);
            self.router_buff_mappings_to_execute.clear();
        }
    }

    fn dedup_and_shuffle<T: PartialEq + Ord>(v: &mut Vec<T>, dedup: bool, shuffle: bool) {
        if dedup {
            v.sort();
            v.dedup();
        }
        if shuffle {
            fastrand::shuffle(v);
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
            cms.iter().enumerate().for_each(|(cm_idx, cm)| {
                cm.set_last_known_io(value);
                cm.set_numeric_value(
                    value, /* NB/TODO: for Rel controls in proposed "stable mode": value + cm.get_numeric_value())
                          and safe ptr to the control to zero-out after mappings run complete*/
                );
                if !mappings.is_empty() {
                    self.router_buff_mappings_to_execute.extend(&mappings[cm_idx]);
                }
            });

            let dedup = cms.len() > 1;
            let shuffle = self.router_buff_mappings_to_execute.len() > 1;
            Self::dedup_and_shuffle(&mut self.router_buff_mappings_to_execute, dedup, shuffle);
            self.run_mappings__(device_id);
            self.router_buff_mappings_to_execute.clear();

            if control_type.is_relative() {
                cms.iter().for_each(|cm| cm.set_numeric_value(0.0));
            }
        }
    }

    fn run_mappings__(&self, triggering_device_id: ObjId) {
        for mapping_idx in self.router_buff_mappings_to_execute.iter() {
            let mapping = &self.cfg.mappings[*mapping_idx];
            self.execute_mapping_on_active_input(triggering_device_id, mapping, mapping.src.get_numeric_value());
        }
    }

    fn execute_mapping_on_active_input(
        &self,
        runtime_input_device_id: ObjId,
        mapping: &Mapping,
        input_value: BaseNumT,
    ) {
        mapping.set_last_known_io((Some(input_value), None));
        let final_value = self.apply_transformation_for_mapping(runtime_input_device_id, mapping, input_value, false);
        mapping.set_last_known_io((None, Some(final_value)));

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
        // TODO:? let _ = self.lua.gc_collect().inspect_err(|e| log::error!("{e}"));

        for idx in &self.idle_tick_mappings {
            let mapping = &self.cfg.mappings[*idx];
            if let Some(flag) = mapping.dst.get_idle_tick_enabled_flag()
                && !flag.load(Relaxed)
            {
                continue;
            }

            let idle_in_value = mapping.src.get_numeric_value();

            mapping.set_last_known_io((Some(idle_in_value), None));

            let final_value =
                self.apply_transformation_for_mapping(ObjId::from(usize::MAX), mapping, idle_in_value, true);

            mapping.set_last_known_io((None, Some(final_value)));

            match &mapping.dst {
                ValueDsts::Void => {}
                ValueDsts::Dynamic(d) => {
                    self.set_dyn_value(d, final_value, self.debug_idle_tick.into());
                }
            }
        }
    }

    fn apply_transformation_for_mapping(
        &self,
        runtime_input_device_id: ObjId,
        mapping: &Mapping,
        value: BaseNumT,
        is_idle_tick: bool,
    ) -> BaseNumT {
        let mut vd = MappedValue::<BaseNumT> {
            value,
            interval: mapping.src.get_interval(),
            relativity: mapping.src.get_relativity(),
        };

        if !vd.interval.contains_value_closed(vd.value) {
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

        vd = mapping.transformation.exec(
            vd,
            &MappingTfmExecCtx {
                mapping_engine: self,
                current_mapping_src: &mapping.src,
                current_mapping_dst: &mapping.dst,
                is_idle_tick,
                lua: &self.lua,
            },
        );

        let dst_interval = mapping.dst.get_interval();
        if vd.interval != dst_interval {
            vd.value = dst_interval.map_from(vd.value, &vd.interval, OutOfRangePolicy::WarnAndClamp);
        }

        vd.value
    }

    fn set_dyn_value(&self, d: &DynValueRefs, val: BaseNumT, debug: DebugLevel) {
        match d {
            DynValueRefs::DeviceControlMatcher(d) => {
                // NB/TODO: for Rel controls in proposed "stable mode": do not reset those buffers
                // NB/TODO: just emit event for the value to be re-fed into engine later
                d.control_matcher.set_last_known_io(val);
                d.control_matcher.set_numeric_value(val);

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
            DynValueRefs::Variable(v) => v.variable.value.store(v.variable.interval.clamp(val), Relaxed),
        }
    }
}

pub(crate) struct MappingTfmExecCtx<'m, 'driver_loop> {
    mapping_engine: &'m MappingEngine<'driver_loop>,
    #[allow(unused)]
    current_mapping_src: &'m ValueSrcs,
    current_mapping_dst: &'m ValueDsts,
    is_idle_tick: bool,
    lua: &'m mlua::Lua,
}

impl<'m, 'driver_loop> TfmExecCtx for MappingTfmExecCtx<'m, 'driver_loop> {
    fn get_main_dst(&self) -> &ValueDsts {
        self.current_mapping_dst
    }

    fn get_ff_x(&self, dk: &str) -> BaseNumT {
        self.mapping_engine.hid_mgr.ff_get_x_sum_symm_norm(dk)
    }

    fn get_ff_y(&self, dk: &str) -> BaseNumT {
        self.mapping_engine.hid_mgr.ff_get_y_sum_symm_norm(dk)
    }

    fn set_dyn_value(&self, dst: &DynValueRefs, v: BaseNumT) {
        self.mapping_engine.set_dyn_value(dst, v, get_debug_level());
    }

    fn set_ff_x_axis_pos(&self, dk: &str, ck: &str, ivl: NumInterval<BaseNumT>) {
        self.mapping_engine.hid_mgr.ff_set_x_axis_pos(dk, ck, ivl);
    }

    fn set_ff_y_axis_pos(&self, dk: &str, ck: &str, ivl: NumInterval<BaseNumT>) {
        self.mapping_engine.hid_mgr.ff_set_y_axis_pos(dk, ck, ivl);
    }

    fn is_idle_tick(&self) -> bool {
        self.is_idle_tick
    }

    fn get_idle_tick_rate(&self) -> u32 {
        self.mapping_engine.get_idle_tick_rate()
    }

    fn get_lua(&self) -> &mlua::Lua {
        self.lua
    }
}
