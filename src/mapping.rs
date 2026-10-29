use crate::base_num::BaseAtomicT;
use crate::base_num::BaseNumT;
use crate::hid_device::HidDeviceEvent;

use crate::debug::DebugLevel;
use crate::debug::get_debug_level;
use crate::device_and_device_manager::CommonDeviceCfgIface;
use crate::device_and_device_manager::{DeviceKind, OpenedDeviceInfoIface};
use crate::device_and_device_manager::{DeviceManagerCommon, DeviceManagerWithFfb};
use crate::mapped_controls::MappedCtls;
#[cfg(feature = "midi")]
use crate::midi::MidiDeviceEvent;
use crate::schemas_cfg::Config;
use crate::schemas_common::ControlCfgCommonIface;
use crate::schemas_common::{ObjId, WithRuntimeId};
use crate::schemas_control_matcher::ControlMatchers;

use crate::schemas_hid::HidDeviceCfg;
use crate::schemas_mapping::MapperMode;
use crate::schemas_mapping::Mapping;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiMatcherCfg;
use crate::schemas_transform::{DynValFilter, collect_dynamic_value_matchers};
use crate::schemas_value::WithNumericValue;
use crate::schemas_value::{
    DeviceControlMatcherRef, ValueDsts, WithDeviceControlMatcherRef, WithLastKnownIOSettable, WithNumInterval,
    WithNumericValueSettable,
};
use crate::schemas_value::{ValueSrcs, WithRelativity};

use crate::stats::StatsIface;
use crate::tfm_exec::get_out_of_range_policy;
use crate::tfm_exec::{TfmExecCtx, WithTfmExec};
use anyhow::Result;
use crossbeam_utils::CachePadded;
use log::{debug, info, warn};
// use num_traits::Zero;
use std::collections::HashMap;
use std::fs;
#[cfg(not(feature = "midi"))]
use std::marker::PhantomData;
use std::ops::Not;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use tokio::select;
use tokio::time::{MissedTickBehavior, interval};

// ---------------------------------------------
const EMERGENCY_THROTTLE_DURATION_SEC_INC: BaseNumT = 0.1 as BaseNumT;
const EMERGENCY_THROTTLE_DURATION_SEC_MIN: BaseNumT = 0.01 as BaseNumT;
const EMERGENCY_THROTTLE_DURATION_SEC_MAX: BaseNumT = 15.0 as BaseNumT;
const EMERGENCY_THROTTLE_TIMEOUT_RESET_AFTER_PROCESSED_EVENTS_NUM: usize = 10000;

// ---------------------------------------------
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) enum MappingEngineCmd {
    _None,
    #[default]
    UpdateMappingRouterIdleTickOnly,
    UpdateMappingRouter,
}

// ---------------------------------------------
pub(crate) trait WithMappingTriggerPredicate {
    type TriggersMappingIn;
    fn triggers_mapping(&self, ctx: Self::TriggersMappingIn) -> bool;
}

// ---------------------------------------------
pub(crate) trait MappedHidManager:
    DeviceManagerCommon<DeviceCfgT = HidDeviceCfg, DeviceEventT = HidDeviceEvent> + DeviceManagerWithFfb
{
}

// ---------------------------------------------
#[cfg(feature = "midi")]
pub(crate) trait MappedMidiManager:
    DeviceManagerCommon<DeviceCfgT = MidiMatcherCfg, DeviceEventT = MidiDeviceEvent>
{
}

// ---------------------------------------------

pub(crate) struct MappingEngine<
    'd,
    HidManagerT: MappedHidManager,
    #[cfg(feature = "midi")] MidiManagerT: MappedMidiManager,
    #[cfg(not(feature = "midi"))] MidiManagerT,
> {
    running: bool,
    // ---
    debug: DebugLevel,
    debug_idle_tick: bool,
    // ---
    cfg: Config,
    hid_mgr: &'d HidManagerT,
    #[cfg(feature = "midi")]
    midi_mgr: &'d MidiManagerT,
    #[cfg(not(feature = "midi"))]
    midi_mgr_placeholder: PhantomData<MidiManagerT>,
    // ---
    //  Mapping router algorithm index and runtime buffer.
    // ---
    #[allow(clippy::type_complexity)]
    router_index_sysdev_and_ctl_type_to_cms_and_mappings:
        HashMap<(ObjId, MappedCtls), (Vec<ControlMatchers>, Vec<Vec<usize>>)>,
    scheduled_mappings_immediate: Vec<usize>,
    scheduled_mappings_normal: Vec<usize>,
    rel_ctls: Vec<Arc<CachePadded<BaseAtomicT>>>,
    // ---
    idle_tick_mappings: Vec<usize>,
    #[cfg(feature = "lua")]
    lua: &'d mlua::Lua,
    mapped_events_dt_stats: MappingEngineStats,
}

pub(crate) trait Mapper<'d> {
    type HidManagerT: MappedHidManager;
    #[cfg(feature = "midi")]
    type MidiManagerT: MappedMidiManager;

    fn new(
        debug: DebugLevel,
        debug_idle_tick: bool,
        cfg: Config,
        hid_mgr: &'d Self::HidManagerT,
        #[cfg(feature = "midi")] midi_mgr: &'d Self::MidiManagerT,
        #[cfg(feature = "lua")] lua: &'d mlua::Lua,
    ) -> Result<Self>
    where
        Self: Sized;

    fn set_cfg(&mut self, cfg: Config);
    fn set_mappings(&mut self, mappings: &[Mapping]);
    fn active_mappings_count(&self) -> usize;

    fn set_mode(&mut self, mode: MapperMode);
    fn get_mode(&self) -> &MapperMode;

    fn init(&mut self) -> Result<()>;
    fn idle_tick_mappings_reset(&mut self);

    async fn run(&mut self);
    fn stop(&mut self) -> Result<()>;
}

impl<
    'd,
    HidManagerT: MappedHidManager,
    #[cfg(feature = "midi")] MidiManagerT: MappedMidiManager,
    #[cfg(not(feature = "midi"))] MidiManagerT,
> Drop for MappingEngine<'d, HidManagerT, MidiManagerT>
{
    fn drop(&mut self) {
        let _ = self
            .stop()
            .inspect_err(|e| log::error!("Error on mapping engine shutdown: {e}"));
    }
}

#[derive(Clone, Copy)]
enum MapppingSchedContext {
    Normal,
    Idle,
    Immediate,
}

impl MapppingSchedContext {
    fn is_idle(&self) -> bool {
        matches!(self, MapppingSchedContext::Idle)
    }
    #[allow(unused)]
    fn is_immediate(&self) -> bool {
        matches!(self, MapppingSchedContext::Immediate)
    }
    fn is_normal(&self) -> bool {
        matches!(self, MapppingSchedContext::Normal)
    }
}

impl<
    'd,
    HidManagerT: MappedHidManager,
    #[cfg(feature = "midi")] MidiManagerT: MappedMidiManager,
    #[cfg(not(feature = "midi"))] MidiManagerT,
> Mapper<'d> for MappingEngine<'d, HidManagerT, MidiManagerT>
{
    type HidManagerT = HidManagerT;
    #[cfg(feature = "midi")]
    type MidiManagerT = MidiManagerT;

    #[allow(clippy::too_many_arguments)]
    fn new(
        debug: DebugLevel,
        debug_idle_tick: bool,
        cfg: Config,
        hid_mgr: &'d Self::HidManagerT,
        #[cfg(feature = "midi")] midi_mgr: &'d Self::MidiManagerT,
        #[cfg(feature = "lua")] lua: &'d mlua::Lua,
    ) -> Result<Self> {
        Ok(Self {
            // ---
            running: false,
            // ---
            debug,
            debug_idle_tick,
            // ---
            cfg,
            hid_mgr,
            #[cfg(feature = "midi")]
            midi_mgr,
            #[cfg(not(feature = "midi"))]
            midi_mgr_placeholder: PhantomData,
            // ---
            router_index_sysdev_and_ctl_type_to_cms_and_mappings: Default::default(),
            scheduled_mappings_immediate: Default::default(),
            scheduled_mappings_normal: Default::default(),
            rel_ctls: Default::default(),
            // ---
            idle_tick_mappings: Default::default(),
            #[cfg(feature = "lua")]
            lua,
            mapped_events_dt_stats: MappingEngineStats::default()
                .with_mean_alpha(0.7)
                .with_threshold_from(1e-6),
        })
    }

    fn set_cfg(&mut self, cfg: Config) {
        self.cfg = cfg;
        self.reset_rel_ctls_cache();
    }

    fn set_mappings(&mut self, mappings: &[Mapping]) {
        self.cfg.mappings = mappings.to_vec();
    }

    fn active_mappings_count(&self) -> usize {
        self.cfg.mappings.iter().filter(|m| *m.enabled).count()
    }

    // In order to provide shorter names for variables, the following acronims are used:
    // sysdev: system device: a device available in current system including virtual ones.
    // dmk: Device matcher key (a config key with which we reference a device mather, used as 'device: "mydevice"').
    // dm: Device matcher.
    // cmk: Control matcher key.
    // cm: Control matcher.
    fn init(&mut self) -> Result<()> {
        info!("Initializing mapping engine router.");

        // +++++++++++++++++++++++++++++++++++++++++++++++++++++++++
        self.idle_tick_mappings_reset();
        self.router_index_sysdev_and_ctl_type_to_cms_and_mappings.clear();
        self.scheduled_mappings_normal.clear();
        self.reset_rel_ctls_cache();

        // +++++++++++++++++++++++++++++++++++++++++++++++++++++++++
        let mut info_sysdev_to_enabled_mappings: HashMap<ObjId, Vec<usize>> = Default::default();

        let mut collect_enabled_mappings_for_dmk_and_cm =
            |dmk: &str, cm_id: ObjId, cm_idx, mappings: &mut Vec<Vec<usize>>, opened_device_id: ObjId| {
                if mappings.get(cm_idx).is_none() {
                    mappings.resize(cm_idx + 1, Vec::new());
                };

                self.cfg
                    .mappings
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| *m.enabled)
                    .for_each(|(mapping_idx, mapping)| {
                        collect_dynamic_value_matchers(mapping, |ctx| {
                            ctx.contains(DynValFilter::Control | DynValFilter::Src)
                        })
                        .iter()
                        .map(|v| {
                            v.get_device_control_matcher_ref()
                                .expect("Only device control matchers expected here")
                                .clone()
                        })
                        .filter(|src| {
                            src.device_matcher_key == *dmk
                                && src.control_matcher.get_id() == cm_id
                                && src.triggers_mapping(())
                        })
                        .collect::<Vec<_>>()
                        .is_empty()
                        .not()
                        .then(|| {
                            let cm_sourcing_mappings: &mut Vec<usize> = mappings.get_mut(cm_idx).unwrap();
                            cm_sourcing_mappings.push(mapping_idx);
                            cm_sourcing_mappings.sort();
                            cm_sourcing_mappings.dedup();

                            info_sysdev_to_enabled_mappings
                                .entry(opened_device_id)
                                .or_default()
                                .push(mapping_idx);
                        });
                    });
            };

        macro_rules! setup_mapping_metadata_for {
            ($available_device_info:ident, $cfg:expr, $device_manager:expr, $cms_variant:expr) => {{
                for (dmk, dm) in $cfg
                    .iter()
                    .filter(|(_, device_cfg)| {
                        device_cfg.is_enabled() && device_cfg.matches_available_device($available_device_info)
                    })
                    .collect::<Vec<(_, _)>>()
                {
                    let opened_device_id = $device_manager
                        .open_device($available_device_info, dmk, dm)?
                        .get_opened_device_id();

                    for cm in dm.controls.values() {
                        let (cms, mappings) = self
                            .router_index_sysdev_and_ctl_type_to_cms_and_mappings
                            .entry((opened_device_id, cm.get_ctl_type()))
                            .or_default();
                        cms.push($cms_variant(cm.clone()));
                        collect_enabled_mappings_for_dmk_and_cm(
                            dmk,
                            cm.get_id(),
                            cms.len() - 1,
                            mappings,
                            opened_device_id,
                        );
                    }
                }
            }};
        }

        for available_hid_device_info in &self.hid_mgr.enumerate_available_devices(Some(
            (DeviceKind::Mouse | DeviceKind::Keyboard | DeviceKind::Gamepad | DeviceKind::Joystick).into(),
        )) {
            setup_mapping_metadata_for!(
                available_hid_device_info,
                self.cfg.devices.hid,
                self.hid_mgr,
                ControlMatchers::Hid
            );
        }

        #[cfg(feature = "midi")]
        for available_midi_device_info in &self.midi_mgr.enumerate_available_devices(None) {
            setup_mapping_metadata_for!(
                available_midi_device_info,
                self.cfg.devices.midi,
                self.midi_mgr,
                ControlMatchers::Midi
            );
        }

        self.router_index_sysdev_and_ctl_type_to_cms_and_mappings
            .retain(|_, v| v.1.iter().any(|v| !v.is_empty()));

        for v in info_sysdev_to_enabled_mappings.values_mut() {
            v.sort();
            v.dedup();
        }

        // ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info!(
            "Mapping Router built. Mapped source devices: {}",
            info_sysdev_to_enabled_mappings.len()
        );

        if self.debug.is_on() {
            let _ = fs::write(
                format!("{}.mapping_router_debug.txt", crate::config::APP_NAME),
                format!("{:#?}", self.router_index_sysdev_and_ctl_type_to_cms_and_mappings),
            );
        }

        // Run all enabled mappings once on init.
        self.run_mappings(
            &self
                .cfg
                .mappings
                .iter()
                .enumerate()
                .filter(|(_, m)| *m.enabled)
                .map(|(i, _)| i)
                .collect::<Vec<_>>(),
            MapppingSchedContext::Normal,
        );

        Ok(())
    }

    fn idle_tick_mappings_reset(&mut self) {
        self.idle_tick_mappings.clear();
        self.cfg
            .mappings
            .iter()
            .enumerate()
            .filter(|(_, mapping)| *mapping.enabled && *mapping.run_on_idle)
            .for_each(|(idx, _)| self.idle_tick_mappings.push(idx));
    }

    async fn run(&mut self) {
        self.running = true;

        let mode = self.cfg.global.mode;
        let is_reactive: bool = mode.is_reactive();
        let is_capped: bool = mode.is_capped();
        let target_mapping_tick_dt = mode.calc_mapping_tick_period();

        let mut idle_ticker = interval(mode.calc_idle_tick_period());
        let mut mapping_ticker = interval(target_mapping_tick_dt);

        let mut events_count: usize = 0;
        let mut last_throttle_at_events_count: usize = 0;

        idle_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        mapping_ticker.set_missed_tick_behavior(MissedTickBehavior::Burst);

        //-------------------------------- MAIN LOOP ----------------------------------
        const DBG_MAIN_LOOP_TIMING: bool = false;
        const DBG_CAPPED_MODE: bool = false;

        let mut current_throttle_duration =
            std::time::Duration::from_secs_f32(EMERGENCY_THROTTLE_DURATION_SEC_MIN as f32);

        let main_loop_processing_time_stats = MappingEngineStats::default();
        let mut last_event_time = std::time::Instant::now();

        while self.running {
            let main_loop_iter_start = std::time::Instant::now();

            let mut update_mapped_events_dt_stat = |stats: &MappingEngineStats| {
                stats.record((main_loop_iter_start - last_event_time).as_secs_f32() as BaseNumT);
                last_event_time = main_loop_iter_start;
            };

            select! {
            biased;
            _ = mapping_ticker.tick(), if !is_reactive => self.process_and_run_mappings(MapppingSchedContext::Normal),
            _ = async {
                select! {
                    event = {#[cfg(feature = "midi")] {self.midi_mgr.consume_any_opened_device_event()}
                             #[cfg(not(feature = "midi"))] std::future::pending::<()>()} =>
                        {#[cfg(feature = "midi")] {
                        match event {
                            Some(event) =>
                                if self.on_midi_event(event) {
                                    update_mapped_events_dt_stat(&self.mapped_events_dt_stats);
                                },
                            None => { log::error!("MIDI manager connection is gone");
                            self.running = false; }}}},
                    event = self.hid_mgr.consume_any_opened_device_event() =>
                        match event {
                            Some(event) =>
                                if self.on_hid_event(event) {
                                    update_mapped_events_dt_stat(&self.mapped_events_dt_stats);
                                },
                            None => { log::error!("HID manager connection is gone");
                            self.running = false; }},
                }
            } => {
                (last_throttle_at_events_count, current_throttle_duration) = self
                    .emergency_throttling(last_throttle_at_events_count, events_count, current_throttle_duration)
                    .await;

                self.process_and_run_mappings(MapppingSchedContext::Immediate);
                if is_reactive {
                    self.process_and_run_mappings(MapppingSchedContext::Normal);
                } else if is_capped {
                    if self.mapped_events_dt_stats.get_mean() > target_mapping_tick_dt.as_secs_f32() as BaseNumT  {
                        self.process_and_run_mappings(MapppingSchedContext::Normal);
                        if DBG_CAPPED_MODE { dbg!("Capped mode: mappings run"); }
                    } else if DBG_CAPPED_MODE {
                        dbg!("Capped mode: mappings throttled");
                        dbg!(&self.mapped_events_dt_stats);
                    }
                }

                events_count = events_count.wrapping_add(1);
            },
            _ = idle_ticker.tick() => self.process_and_run_mappings(MapppingSchedContext::Idle)
            }

            if DBG_MAIN_LOOP_TIMING {
                dbg!(&main_loop_processing_time_stats, target_mapping_tick_dt);
            }

            main_loop_processing_time_stats
                .record((std::time::Instant::now() - main_loop_iter_start).as_secs_f32() as BaseNumT);
        }
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        Ok(())
    }

    fn set_mode(&mut self, mode: MapperMode) {
        self.cfg.global.mode = mode;
    }

    fn get_mode(&self) -> &MapperMode {
        &self.cfg.global.mode
    }
}

#[derive(Clone, Debug, Default)]
struct MappingEngineStats {
    threshold_from: BaseNumT,
    mean_alpha: BaseNumT,
    last: std::cell::Cell<BaseNumT>,
    mean: std::cell::Cell<BaseNumT>,
    min: std::cell::Cell<BaseNumT>,
    max: std::cell::Cell<BaseNumT>,
}

impl StatsIface for MappingEngineStats {
    type In = BaseNumT;
    type Out = BaseNumT;

    fn record(&self, v: Self::In) {
        if v >= self.threshold_from {
            self.mean
                .set(self.mean.get() * (1.0 - self.mean_alpha) + v * self.mean_alpha);
            if self.max.get() < v {
                self.max.set(v);
            }
            if self.min.get() == 0.0 || self.min.get() > v {
                self.min.set(v);
            }
            self.last.set(v);
        }
    }

    fn get_mean(&self) -> Self::Out {
        self.mean.get()
    }

    fn get_max(&self) -> Self::Out {
        self.max.get()
    }

    fn get_min(&self) -> Self::Out {
        self.min.get()
    }

    fn get_last(&self) -> Self::Out {
        self.last.get()
    }

    fn with_threshold_from(mut self, threshold: Self::In) -> Self {
        self.threshold_from = threshold;
        self
    }

    fn with_mean_alpha(mut self, alpha: Self::In) -> Self {
        self.mean_alpha = alpha;
        self
    }
}

impl<
    'd,
    HidManagerT: MappedHidManager,
    #[cfg(feature = "midi")] MidiManagerT: MappedMidiManager,
    #[cfg(not(feature = "midi"))] MidiManagerT,
> MappingEngine<'d, HidManagerT, MidiManagerT>
{
    fn reset_rel_ctls_cache(&mut self) {
        let mut rel_ctls: Vec<_> = Default::default();
        self.cfg.devices.hid.iter().for_each(|d| {
            rel_ctls.extend(
                d.1.controls
                    .values()
                    .filter(|c| c.r#type.is_relative())
                    .map(|c| c.current_value_ptr_ref().clone())
                    .collect::<Vec<_>>(),
            );
        });
        self.rel_ctls = rel_ctls;
    }

    // TODO: idle tick shadowing.
    fn set_idle_tick_enabled_on_device_control_for_mapping(&self, mapping: &Mapping) {
        if let Some(flag) = mapping.dst.get_idle_tick_enabled_flag()
            && let Ok(prev) = flag.compare_exchange(
                !*mapping.run_on_idle,
                *mapping.run_on_idle,
                // Yes, both relaxed because active mapping will retrigger this, no need for stronger.
                Relaxed,
                Relaxed,
            )
            && self.debug.is_on()
        {
            debug!(
                "Idle-tick update for {}: {} -> {}",
                mapping.dst, prev, mapping.run_on_idle
            );
        }
    }

    #[cfg(feature = "midi")]
    #[must_use]
    fn on_midi_event(&mut self, event: MidiDeviceEvent) -> bool {
        let mut mapped = false;
        if let Some((cms, mappings)) = self
            .router_index_sysdev_and_ctl_type_to_cms_and_mappings
            .get(&(event.device_id, event.data.message_type.into()))
        {
            cms.iter()
                .enumerate()
                .filter(|(_, cm)| {
                    let ControlMatchers::Midi(cm) = cm else { unreachable!() };
                    event.data.matches_control_matcher(cm)
                })
                .for_each(|(cm_idx, cm)| {
                    cm.set_numeric_value(event.data.get_operational_value());

                    if cm.triggers_mapping(()) && !mappings[cm_idx].is_empty() {
                        mapped = true;
                        if event.data.message_type.is_a_button() {
                            self.scheduled_mappings_immediate.extend(&mappings[cm_idx]);
                        } else {
                            self.scheduled_mappings_normal.extend(&mappings[cm_idx]);
                        }
                    }
                });
        }
        mapped
    }

    #[must_use]
    fn on_hid_event(&mut self, event: HidDeviceEvent) -> bool {
        let mut mapped = false;
        if let Some((cms, mappings)) = self
            .router_index_sysdev_and_ctl_type_to_cms_and_mappings
            .get(&(event.device_id, event.data.control_type))
        {
            cms.iter().enumerate().for_each(|(cm_idx, cm)| {
                cm.set_last_known_io(event.data.value);
                cm.set_numeric_value(if cm.get_relativity().is_relative() {
                    cm.get_numeric_value() + event.data.value
                } else {
                    event.data.value
                });

                if cm.triggers_mapping(()) && !mappings[cm_idx].is_empty() {
                    mapped = true;
                    if event.data.control_type.is_button() || event.data.control_type.is_key() {
                        self.scheduled_mappings_immediate.extend(&mappings[cm_idx]);
                    } else {
                        self.scheduled_mappings_normal.extend(&mappings[cm_idx]);
                    }
                }
            });
        }
        mapped
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

    fn run_mappings(&self, mappings: &[usize], sched_ctx: MapppingSchedContext) {
        for mapping_idx in mappings.iter() {
            let mapping = &self.cfg.mappings[*mapping_idx];

            if sched_ctx.is_idle() {
                // TODO: idle tick shadowing.
                if let Some(flag) = mapping.dst.get_idle_tick_enabled_flag()
                    && !flag.load(Relaxed)
                {
                    continue;
                }
            }

            let input_value = mapping.src.get_numeric_value();

            mapping.set_last_known_io((Some(input_value), None));
            let final_value = self.apply_transformation_for_mapping(mapping, input_value, sched_ctx);
            mapping.set_last_known_io((None, Some(final_value)));

            if let Some(d) = &mapping.dst.get_device_control_matcher_ref() {
                self.dcm_write_to_devices(
                    d,
                    final_value,
                    if sched_ctx.is_idle() {
                        self.debug_idle_tick.into()
                    } else {
                        self.debug
                    },
                );
                self.set_idle_tick_enabled_on_device_control_for_mapping(mapping);
            } else {
                mapping.dst.set_numeric_value(final_value);
            }

            if self.debug.is_on() {
                debug!("Mapped {} ({mapping}): {input_value} -> {final_value}", mapping.name);
            }
        }
    }

    fn clear_rel_ctls(&self) {
        self.rel_ctls.iter().for_each(|ctl| {
            ctl.store(0.0, Relaxed);
        });
    }

    fn mappings_by_ctx_mut(&mut self, sched_ctx: MapppingSchedContext) -> &mut Vec<usize> {
        match sched_ctx {
            MapppingSchedContext::Idle => &mut self.idle_tick_mappings,
            MapppingSchedContext::Normal => &mut self.scheduled_mappings_normal,
            MapppingSchedContext::Immediate => &mut self.scheduled_mappings_immediate,
        }
    }

    fn mappings_by_ctx_ref(&self, sched_ctx: MapppingSchedContext) -> &Vec<usize> {
        match sched_ctx {
            MapppingSchedContext::Idle => &self.idle_tick_mappings,
            MapppingSchedContext::Normal => &self.scheduled_mappings_normal,
            MapppingSchedContext::Immediate => &self.scheduled_mappings_immediate,
        }
    }

    fn process_and_run_mappings(&mut self, sched_ctx: MapppingSchedContext) {
        let mappings = self.mappings_by_ctx_mut(sched_ctx);
        if mappings.is_empty() {
            return;
        }

        if sched_ctx.is_idle() {
            if branches::unlikely(!(self.scheduled_mappings_normal.is_empty())) {
                return;
            }
            debug_assert!(
                self.scheduled_mappings_immediate.is_empty() && self.scheduled_mappings_normal.is_empty(),
                "All the input-triggered mappings must be executed by the time of idle tick processing."
            );
            if !self.get_mode().is_reactive() {
                self.clear_rel_ctls();
            } else {
                debug_assert!(
                    self.rel_ctls.iter().all(|ctl| ctl.load(Relaxed) == 0.0 as BaseNumT),
                    "All the relative control matcher values must be reset to 0 (after mappings execution) by the time idle tick processing is triggered."
                );
            }
        } else {
            Self::dedup_and_shuffle(mappings, true, mappings.len() > 1);
        }

        self.run_mappings(self.mappings_by_ctx_ref(sched_ctx), sched_ctx);

        if !sched_ctx.is_idle() {
            self.mappings_by_ctx_mut(sched_ctx).clear();
        }

        if self.get_mode().is_reactive() || sched_ctx.is_normal() {
            self.clear_rel_ctls();
        }
    }

    fn apply_transformation_for_mapping(
        &self,
        mapping: &Mapping,
        mut value: BaseNumT,
        sched_ctx: MapppingSchedContext,
    ) -> BaseNumT {
        if mapping.src.get_relativity().is_absolute() && !mapping.src.get_interval().contains_value_closed(value) {
            warn!(
                "The value (={value}) read from device {} is out of configured interval ({:?}), clamping it.",
                if matches!(sched_ctx, MapppingSchedContext::Idle) {
                    "idle tick"
                } else {
                    "input-triggered mapping"
                },
                mapping.src.get_interval()
            );
            value = mapping.src.get_interval().clamp(value);
        }

        value = mapping.transformation.exec(
            value,
            &MappingTfmExecCtx {
                mapping_engine: self,
                current_mapping_src: Some(&mapping.src),
                current_mapping_dst: Some(&mapping.dst),
                #[cfg(feature = "lua")]
                lua: Some(self.lua),
                sched_ctx,
            },
        );

        let mapping_dst_interval = mapping.dst.get_interval();
        let tfm_out_interval = mapping.transformation.get_out_interval();

        // TODO: Currently only two points of writeback are possible: mapping destination and
        //        script tfm destination ports. The below remapping+clamp is only done for mapping dst,
        //        but not for script dst, where user is supposed to do it manually.
        //        In case more writeback points (e.g. write/tee tfm step) are added,
        //        will need to move this logic to common writeback routine.
        if tfm_out_interval != mapping_dst_interval {
            value = mapping_dst_interval.map_from(
                value,
                &tfm_out_interval,
                get_out_of_range_policy(
                    mapping.transformation.get_out_relativity(),
                    self.get_mode().is_reactive(),
                ),
            );
        }

        // TODO:
        //
        // Reinterpreting relative output as absolute control position
        // e.g. in case of a joystick axis may or may not be feasible.
        // for a physical device that could result in abrupt movements.
        //
        // Currently it is allowed.
        // The mode of "clever" remapping of Rel to Abs is being proposed.
        // Alternatively, we may further mark such conigurations as invalid,
        // discarding writebacks.
        //
        // While allowed, in non-reactive mode relative delta can overshoot the
        // range of abs dst control which may or may not be handled properly
        // by the further processing logic.
        if !self.get_mode().is_reactive()
            && mapping.transformation.get_out_relativity().is_relative()
            && mapping.dst.get_relativity().is_absolute()
        {
            value = mapping_dst_interval.clamp(value);
        }

        value
    }

    fn dcm_write_to_devices(&self, d: &DeviceControlMatcherRef, value: BaseNumT, debug: DebugLevel) {
        // TODO !!!: stable mode: for relative deltas do not reset those buffers
        // TODO !!!: stable mode: just emit event for the value to be re-fed into engine later
        d.control_matcher.set_last_known_io(value);
        d.control_matcher.set_numeric_value(value);

        match d.control_matcher {
            #[cfg(feature = "midi")]
            ControlMatchers::Midi(_) => {
                log::warn!(
                    "MIDI is not yet supported as a destination device. Only supporting variables and HID destinations."
                )
            }
            // TODO !!!: stable mode: for relative deltas out of bounds of estimated range emit multiple events.
            ControlMatchers::Hid(_) => self.hid_mgr.set_control_matcher_and_broadcast(
                &d.device_matcher_key,
                &d.control_matcher_key,
                value,
                !debug.is_on(),
            ),
        }
    }

    async fn emergency_throttling(
        &self,
        mut last_throttle_at_events_count: usize,
        events_count: usize,
        mut current_throttle_duration: std::time::Duration,
    ) -> (usize, std::time::Duration) {
        if events_count > 0
            && let Some(throttle_reason) = || -> Option<&'static str> {
                #[cfg(feature = "midi")]
                if branches::unlikely(self.midi_mgr.events_queue_is_full()) {
                    self.midi_mgr.drain_events_queue();
                    return "Throttling by incoming MIDI events backlog (events queue is full)".into();
                }
                if branches::unlikely(self.hid_mgr.events_queue_is_full()) {
                    self.hid_mgr.drain_events_queue();
                    return "Throttling by incoming HID events backlog (events queue is full)".into();
                }
                return None;
            }()
        {
            branches::mark_unlikely();
            let msg = format!(
                "\n-------\n{throttle_reason}, events will be dropped! Sleeping for {} seconds and draining events queue.\n\
                Mappings cause execution loop, check your mapping configuration!\n\
                {}",
                current_throttle_duration.as_secs_f32(),
                if self.get_mode().is_reactive() {
                    "In case you are using high-frequency gaming mouse, \
                        try switching to Stable or Capped execution mode (current mode is Reactive)"
                } else {
                    ""
                }
            );
            log::warn!("{msg}");
            println!("{msg}");
            if events_count.wrapping_sub(last_throttle_at_events_count)
                > EMERGENCY_THROTTLE_TIMEOUT_RESET_AFTER_PROCESSED_EVENTS_NUM
            {
                current_throttle_duration =
                    std::time::Duration::from_secs_f32(EMERGENCY_THROTTLE_DURATION_SEC_MIN as f32);
            }
            last_throttle_at_events_count = events_count;
            tokio::time::sleep(current_throttle_duration).await;
            current_throttle_duration = std::time::Duration::from_secs_f32(
                (current_throttle_duration.as_secs_f32() + EMERGENCY_THROTTLE_DURATION_SEC_INC as f32)
                    .min(EMERGENCY_THROTTLE_DURATION_SEC_MAX as f32),
            );
        }
        (last_throttle_at_events_count, current_throttle_duration)
    }
}

pub(crate) struct MappingTfmExecCtx<
    'm,
    'd,
    HidManagerT: MappedHidManager,
    #[cfg(feature = "midi")] MidiManagerT: MappedMidiManager,
    #[cfg(not(feature = "midi"))] MidiManagerT,
> {
    mapping_engine: &'m MappingEngine<'d, HidManagerT, MidiManagerT>,
    #[allow(unused)]
    current_mapping_src: Option<&'m ValueSrcs>,
    current_mapping_dst: Option<&'m ValueDsts>,
    sched_ctx: MapppingSchedContext,
    #[cfg(feature = "lua")]
    lua: Option<&'m mlua::Lua>,
}

impl<
    'm,
    'd,
    HidManagerT: MappedHidManager,
    #[cfg(feature = "midi")] MidiManagerT: MappedMidiManager,
    #[cfg(not(feature = "midi"))] MidiManagerT,
> TfmExecCtx for MappingTfmExecCtx<'m, 'd, HidManagerT, MidiManagerT>
{
    fn get_events_dt_stats(&self) -> Option<&impl StatsIface> {
        Some(&self.mapping_engine.mapped_events_dt_stats)
    }

    fn is_immediate_mapping(&self) -> bool {
        matches!(self.sched_ctx, MapppingSchedContext::Immediate)
    }

    fn is_reactive_mode(&self) -> bool {
        self.mapping_engine.get_mode().is_reactive()
    }

    fn get_main_dst(&self) -> Option<&ValueDsts> {
        self.current_mapping_dst
    }

    fn get_ff_x(&self, dk: &str) -> BaseNumT {
        self.mapping_engine.hid_mgr.ff_get_x_sum_symm_norm(dk)
    }

    fn get_ff_y(&self, dk: &str) -> BaseNumT {
        self.mapping_engine.hid_mgr.ff_get_y_sum_symm_norm(dk)
    }

    fn device_control_matcher_ref_write(&self, dcm: &DeviceControlMatcherRef, value: BaseNumT) {
        self.mapping_engine.dcm_write_to_devices(dcm, value, get_debug_level());
    }

    fn is_idle_tick(&self) -> bool {
        matches!(self.sched_ctx, MapppingSchedContext::Idle)
    }

    fn get_idle_tick_rate(&self) -> u32 {
        self.mapping_engine.get_mode().get_idle_tick_rate()
    }

    #[cfg(feature = "lua")]
    fn get_lua(&self) -> Option<&mlua::Lua> {
        self.lua
    }
}
