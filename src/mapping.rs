use crate::base_num::BaseNumT;
use crate::hid_device::HidDeviceEvent;

use crate::debug::DebugLevel;
use crate::debug::get_debug_level;
use crate::device_and_device_manager::WithDeviceClassification;
use crate::device_and_device_manager::{AvailableDeviceInfoIface, DeviceManagerCommon, DeviceManagerWithFfb};
use crate::device_and_device_manager::{DeviceKind, OpenedDeviceInfoIface};
use crate::mapped_controls::MappedCtls;
#[cfg(feature = "midi")]
use crate::midi::MidiDeviceEvent;
use crate::num_interval::{NumInterval, OutOfRangePolicy};
use crate::schemas_cfg::Config;
use crate::schemas_common::{ObjId, WithRuntimeId};
use crate::schemas_control_matcher::ControlMatchers;

use crate::schemas_hid::{HidControlMatcherCfg, HidDeviceCfg};
use crate::schemas_mapping::MapperMode;
use crate::schemas_mapping::Mapping;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiMatcherCfg;
use crate::schemas_transform::{DynValFilter, collect_dynamic_value_matchers};
use crate::schemas_value::{
    DeviceControlMatcherRef, DynValueRefs, ValueDsts, WithDeviceControlMatcherRef, WithLastKnownIOSettable,
    WithNumInterval, WithNumericValueSettable,
};
use crate::schemas_value::{TfmValue, WithNumericValue};
use crate::schemas_value::{ValueSrcs, WithRelativity};

use crate::tfm_exec::{TfmExecCtx, WithTfmExec};
use anyhow::Result;
use log::{debug, info, warn};
// use num_traits::Zero;
use std::collections::HashMap;
use std::fs;
#[cfg(not(feature = "midi"))]
use std::marker::PhantomData;
use std::sync::atomic::Ordering::Relaxed;
use tokio::select;
use tokio::time::{MissedTickBehavior, interval};

// ---------------------------------------------
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) enum MappingEngineCmd {
    _None,
    #[default]
    UpdateMappingRouterIdleTickOnly,
    UpdateMappingRouter,
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
    scheduled_mappings_other: Vec<usize>,
    rel_ctls: Vec<*const HidControlMatcherCfg>,
    // ---
    idle_tick_mappings: Vec<usize>,
    lua: &'d mlua::Lua,
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
        lua: &'d mlua::Lua,
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
        lua: &'d mlua::Lua,
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
            scheduled_mappings_other: Default::default(),
            rel_ctls: Default::default(),
            // ---
            idle_tick_mappings: Default::default(),
            lua,
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

        self.idle_tick_mappings_reset();

        // ---
        self.router_index_sysdev_and_ctl_type_to_cms_and_mappings.clear();
        self.scheduled_mappings_other.clear();

        // +++++++++++++++++++++++++++++++++++++++++++++++++++++++++
        self.reset_rel_ctls_cache();

        // +++++++++++++++++++++++++++++++++++++++++++++++++++++++++
        let available_hid_devices = self.hid_mgr.enumerate_available_devices(Some(
            (DeviceKind::Mouse | DeviceKind::Keyboard | DeviceKind::Gamepad | DeviceKind::Joystick).into(),
        ));

        let mut info_sysdev_to_enabled_mappings: HashMap<ObjId, Vec<usize>> = Default::default();

        let mut collect_enabled_mappings_for_dmk_and_cm =
            |dmk: &str, cm_id: ObjId, cm_idx, mappings: &mut Vec<Vec<usize>>, opened_device_id: ObjId| {
                for (mapping_idx, mapping) in self.cfg.mappings.iter().enumerate().filter(|(_, m)| *m.enabled) {
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

                            info_sysdev_to_enabled_mappings
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
                            .map(|r| r.is_match(available_hid_device_info.get_name()))
                            .or(v.virtual_device_name_ref().map(|n| {
                                crate::hid_device::sanitize_hid_name(n) == available_hid_device_info.get_name()
                            }))
                            .unwrap_or_default()
                        && v.get_classification()
                            .intersects(available_hid_device_info.get_classification())
                })
                .collect::<Vec<(_, _)>>()
            {
                let opened_device_info = self.hid_mgr.open_device(available_hid_device_info, dmk, dm)?;
                let opened_device_id = opened_device_info.get_opened_device_id();

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
        let available_midi_devices = self.midi_mgr.enumerate_available_devices(None);
        #[cfg(feature = "midi")]
        for available_midi_device_info in &available_midi_devices {
            for (dmk, dm) in self
                .cfg
                .devices
                .midi
                .iter()
                .filter(|(_, v)| v.is_enabled() && v.match_name_regex.is_match(available_midi_device_info.get_name()))
                .collect::<Vec<(_, _)>>()
            {
                let opened_device_id = self
                    .midi_mgr
                    .open_device(available_midi_device_info, dmk, dm)?
                    .get_opened_device_id();

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

        // Run all mappings once on init.
        self.scheduled_mappings_other.extend(
            self.cfg
                .mappings
                .iter()
                .enumerate()
                .filter(|(_, m)| *m.enabled)
                .map(|(i, _)| i)
                .collect::<Vec<_>>(),
        );
        self.run_mappings(&self.scheduled_mappings_other);
        self.scheduled_mappings_other.clear();

        Ok(())
    }

    fn idle_tick_mappings_reset(&mut self) {
        self.idle_tick_mappings.clear();
        self.cfg
            .mappings
            .iter()
            .enumerate()
            .filter(|(_, mapping)| *mapping.enabled && mapping.requires_idle_tick)
            .for_each(|(idx, _)| self.idle_tick_mappings.push(idx));
    }

    async fn run(&mut self) {
        self.running = true;

        let mode = self.cfg.global.mode;
        let is_reactive: bool = mode.is_reactive();
        let is_capped: bool = mode.is_capped();
        let mapping_tick_period = mode.calc_mapping_tick_period();

        let mut idle_ticker = interval(mode.calc_idle_tick_period());
        let mut mapping_ticker = interval(mapping_tick_period);

        idle_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        mapping_ticker.set_missed_tick_behavior(MissedTickBehavior::Burst);

        //-------------------------------- MAIN LOOP ----------------------------------
        const DEBUG_MAIN_LOOP_LATENCY: bool = false; // TODO: generalized stats data, observable via Gui.
        let mut last_main_loop_period = std::time::Duration::ZERO;
        while self.running {
            let main_loop_iter_start = std::time::Instant::now();
            select! {
            biased;
            // Any scheduled mappings must execute first.
            _ = mapping_ticker.tick(), if !is_reactive => self.process_mappings_other(),
            // Then we choose between equally prioritized midi or hid  input events.
            _ = async {
                select! {
                    event = {#[cfg(feature = "midi")] {self.midi_mgr.consume_any_opened_device_event()}
                             #[cfg(not(feature = "midi"))] std::future::pending::<()>()}
                    => {#[cfg(feature = "midi")] {
                        match event {Some(event) => self.map_midi_event(event),
                                     None => { log::error!("MIDI manager connection is gone");
                                     self.running = false; }}}},
                    event = self.hid_mgr.consume_any_opened_device_event() =>
                        match event {Some(event) => self.map_hid_event(event),
                                     None => { log::error!("HID manager connection is gone");
                                     self.running = false; }},
                }
            } => {
                // Immediate-priority mappings are being run... immediately, irrespective to execution mode.
                self.process_mappings_immediate();
                // If the mode is reactive or capped and the frequency is below the threshold,
                // execute mappings immediately.
                if is_reactive || (is_capped && last_main_loop_period > mapping_tick_period) {
                    self.process_mappings_other();
                }
            },
            // Lastly we check to run idle tick, which bursts if we skip it due to any of the above.
            _ = idle_ticker.tick() => self.process_idle_tick()
            }

            last_main_loop_period = std::time::Instant::now() - main_loop_iter_start;
            if DEBUG_MAIN_LOOP_LATENCY {
                dbg!(last_main_loop_period, mapping_tick_period);
            }
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
                    .map(|c| c as *const HidControlMatcherCfg)
                    .collect::<Vec<_>>(),
            );
        });
        self.rel_ctls = rel_ctls;
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
    fn map_midi_event(&mut self, event: MidiDeviceEvent) {
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
                    if !mappings.is_empty() {
                        if event.data.message_type.is_a_button() {
                            self.scheduled_mappings_immediate.extend(&mappings[cm_idx]);
                        } else {
                            self.scheduled_mappings_other.extend(&mappings[cm_idx]);
                        }
                    }
                });
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

    fn map_hid_event(&mut self, event: HidDeviceEvent) {
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

                if !mappings.is_empty() {
                    if event.data.control_type.is_button() || event.data.control_type.is_key() {
                        self.scheduled_mappings_immediate.extend(&mappings[cm_idx]);
                    } else {
                        self.scheduled_mappings_other.extend(&mappings[cm_idx]);
                    }
                }
            });
        }
    }

    fn run_mappings(&self, mappings: &Vec<usize>) {
        for mapping_idx in mappings.iter() {
            let mapping = &self.cfg.mappings[*mapping_idx];
            let input_value = mapping.src.get_numeric_value();

            mapping.set_last_known_io((Some(input_value), None));
            let final_value = self.apply_transformation_for_mapping(mapping, input_value, false);
            mapping.set_last_known_io((None, Some(final_value)));

            if let Some(d) = &mapping.dst.get_device_control_matcher_ref() {
                self.dcm_write_to_devices(d, final_value, self.debug);
                self.set_idle_tick_enabled_on_device_control_for_mapping(mapping);
            } else {
                mapping.dst.set_numeric_value(final_value);
            }

            if self.debug.is_on() {
                debug!(
                    "Mapped {} ({}): {} -> {}",
                    mapping.name, mapping, input_value, final_value
                );
            }
        }
    }

    fn clear_rel_ctls(&self) {
        self.rel_ctls.iter().for_each(|ctl| {
            unsafe { &**ctl }.set_numeric_value(0.0);
        });
    }

    fn process_mappings_other(&mut self) {
        if self.scheduled_mappings_other.is_empty() {
            return;
        }
        let dedup = true;
        let shuffle = self.scheduled_mappings_other.len() > 1;
        Self::dedup_and_shuffle(&mut self.scheduled_mappings_other, dedup, shuffle);
        self.run_mappings(&self.scheduled_mappings_other);
        self.scheduled_mappings_other.clear();
        self.clear_rel_ctls();
    }

    fn process_mappings_immediate(&mut self) {
        if self.scheduled_mappings_immediate.is_empty() {
            return;
        }
        let dedup = true;
        let shuffle = self.scheduled_mappings_immediate.len() > 1;
        Self::dedup_and_shuffle(&mut self.scheduled_mappings_immediate, dedup, shuffle);
        self.run_mappings(&self.scheduled_mappings_immediate);
        self.scheduled_mappings_immediate.clear();
        // We do not reset relative controls after processing immediate mappings.
    }

    fn process_idle_tick(&self) {
        if !(self.scheduled_mappings_immediate.is_empty() && self.scheduled_mappings_other.is_empty()) {
            return;
        }
        // self.clear_rel_ctls();
        // debug_assert!(
        //     self.scheduled_mappings_immediate.is_empty() && self.scheduled_mappings_other.is_empty(),
        //     "All the input-triggered mappings must be executed by the time idle tick processing is triggered."
        // );
        // debug_assert!(
        //     self.rel_ctls
        //         .iter()
        //         .all(|ctl| unsafe { &**ctl }.get_numeric_value().is_zero()),
        //     "All the relative control matcher values must be reset to 0 (after mappings execution) by the time idle tick processing is triggered."
        // );
        for idx in &self.idle_tick_mappings {
            let mapping = &self.cfg.mappings[*idx];
            if let Some(flag) = mapping.dst.get_idle_tick_enabled_flag()
                && !flag.load(Relaxed)
            {
                continue;
            }

            let idle_in_value = mapping.src.get_numeric_value();

            mapping.set_last_known_io((Some(idle_in_value), None));

            let final_value = self.apply_transformation_for_mapping(mapping, idle_in_value, true);

            mapping.set_last_known_io((None, Some(final_value)));

            if let Some(d) = &mapping.dst.get_device_control_matcher_ref() {
                self.dcm_write_to_devices(d, final_value, self.debug_idle_tick.into());
            } else {
                mapping.dst.set_numeric_value(final_value);
            }
        }
    }

    fn apply_transformation_for_mapping(&self, mapping: &Mapping, value: BaseNumT, is_idle_tick: bool) -> BaseNumT {
        let mut vd = TfmValue::<BaseNumT> {
            value,
            interval: mapping.src.get_interval(),
            relativity: mapping.src.get_relativity(),
        };

        if vd.relativity.is_absolute() && !vd.interval.contains_value_closed(vd.value) {
            warn!(
                "The value (={}) read from device {} \
                        is out of configured interval ({:?}), clamping it.",
                vd.value,
                if is_idle_tick {
                    "idle tick"
                } else {
                    "input-triggered mapping"
                },
                vd.interval
            );
            vd.value = vd.interval.clamp(vd.value);
        }

        vd = mapping.transformation.exec(
            vd,
            &MappingTfmExecCtx {
                mapping_engine: self,
                current_mapping_src: Some(&mapping.src),
                current_mapping_dst: Some(&mapping.dst),
                is_idle_tick,
                lua: Some(self.lua),
            },
        );

        let dst_interval = mapping.dst.get_interval();
        if vd.interval != dst_interval {
            vd.value = dst_interval.map_from(
                vd.value,
                &vd.interval,
                if vd.relativity.is_absolute() {
                    OutOfRangePolicy::WarnIfDebugAndClamp
                } else {
                    OutOfRangePolicy::Allow
                },
            );
        }

        vd.value
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
    is_idle_tick: bool,
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
        self.mapping_engine.get_mode().get_idle_tick_rate()
    }

    fn get_lua(&self) -> Option<&mlua::Lua> {
        self.lua
    }
}
