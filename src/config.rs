use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use log::info;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex;
use yaml_merge_keys::merge_keys_serde;

use regex::Regex;

use crate::common::NumInterval;
use crate::schemas::*;

pub const APP_VERSION_STR: &str = "3.3";
pub const CONFIG_VERSION_STR: &str = APP_VERSION_STR;
pub const APP_DEFAULT_CONFIG_FILE: &str = "conf/mmvj_cfg.yaml";
pub const APP_DEFAULT_PREDEF_CONFIG_FILE_CFG_RELATIVE: &str = "mmvj_cfg_predefines.yaml";
pub const APP_AUTHORS: &str = "Leonid Satanovskiy and small furry creatures from Alpha Centauri.";
pub const APP_NAME: &str = "MMVJ";
pub const APP_COMMAND_NAME: &str = "mmvj";
pub const APP_LONG_NAME: &str = "Mouse and MIDI to Virtual Joystick (Transfor)Mapper";
pub const APP_ABOUT: &str = APP_LONG_NAME;
pub const APP_LONG_ABOUT: &str = APP_ABOUT;
pub const APP_DEFAULT_NO_HOT_RELOAD: &str = "false";
pub const APP_DEFAULT_LATENCY_STR: &str = "normal";
pub const APP_DEFAULT_MAX_LOG_LEVEL: &str = "debug";
const INCLUDE_YAML_KEY: &str = "_include";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) global: GlobalSettings,
    #[serde(default)]
    pub(crate) midi_devices: Option<HashMap<String, MidiDevice>>,
    #[serde(default)]
    pub(crate) mouse_devices: Option<HashMap<String, MouseDevice>>,
    #[serde(default)]
    pub(crate) virtual_joysticks: HashMap<String, VirtualJoystick>,
    #[serde(default)]
    pub(crate) mappings: Vec<Mapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) created_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_modified: Option<DateTime<Utc>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            global: GlobalSettings::default(),
            midi_devices: None,
            mouse_devices: None,
            virtual_joysticks: HashMap::new(),
            mappings: Vec::new(),
            created_by: Some(APP_LONG_NAME.to_string()),
            created_date: Some(Utc::now()),
            last_modified: Some(Utc::now()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IdleTickRequirementInfo {
    pub(crate) is_required: Option<bool>,
}

impl IdleTickRequirementInfo {
    fn new() -> IdleTickRequirementInfo {
        IdleTickRequirementInfo { is_required: None }
    }
}

#[derive(Debug)]
pub(crate) struct ResolvedMapping {
    pub(crate) name: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) source: ResolvedMappingSource,
    pub(crate) destination: ResolvedMappingDestination,
    pub(crate) transformation: ResolvedTransformation,
    pub(crate) idle_tick_requirement_info__: Mutex<IdleTickRequirementInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedMappingSource {
    pub(crate) device_key: String,
    pub(crate) control_key: String,
    pub(crate) control: ControlReference,
}

#[derive(Debug, Clone)]
pub(crate) enum ControlReference {
    Midi(MidiControl),
    Mouse(ResolvedMouseControl),
}

#[derive(Debug)]
pub(crate) struct ResolvedMappingDestination {
    pub(crate) device_key: String,
    #[allow(dead_code)]
    pub(crate) joystick: VirtualJoystick,
    pub(crate) control_key: String,
    pub(crate) control: ResolvedJoystickControl,
}

impl PartialEq for ResolvedMappingSource {
    fn eq(&self, other: &Self) -> bool {
        self.device_key == other.device_key && self.control_key == other.control_key
    }
}
impl Eq for ResolvedMappingSource {}

impl std::hash::Hash for ResolvedMappingSource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.device_key.hash(state);
        self.control_key.hash(state);
    }
}

impl PartialEq for ResolvedMappingDestination {
    fn eq(&self, other: &Self) -> bool {
        self.device_key == other.device_key && self.control_key == other.control_key
    }
}
impl Eq for ResolvedMappingDestination {}

impl std::hash::Hash for ResolvedMappingDestination {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.device_key.hash(state);
        self.control_key.hash(state);
    }
}

impl PartialEq for ResolvedMapping {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.destination == other.destination
    }
}
impl Eq for ResolvedMapping {}

impl std::hash::Hash for ResolvedMapping {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.hash(state);
        self.destination.hash(state);
    }
}

//------------------------------------------------------
#[derive(Debug)]
pub(crate) struct ConfigManager {
    cfg_file_path_canon: PathBuf,
    _predef_cfg_file_path: PathBuf,
    config: Config,
    predefines: ControlsPredefined,
    _config_changed: bool,
    mappings: Vec<ResolvedMapping>,
    resolved_midi_devices: BTreeMap<String, crate::schemas::ResolvedMidiDevice>,
    resolved_mouse_devices: BTreeMap<String, crate::schemas::ResolvedMouseDevice>,
    resolved_virtual_joysticks: BTreeMap<String, crate::schemas::ResolvedVirtualJoystick>,
    debug: bool,
}

impl ConfigManager {
    pub(crate) fn new(
        cfg_file_path: &Path,
        predef_cfg_file_path: &Path,
        debug: bool,
    ) -> Result<Self> {
        let cfg_file_path_canon =
            fs::canonicalize(cfg_file_path).context("Failed to canonicalize config file path")?;

        let mut predef_cfg_file_path_canon = predef_cfg_file_path.to_path_buf();
        if !predef_cfg_file_path_canon.exists() {
            if let Some(parent) = cfg_file_path_canon.parent() {
                predef_cfg_file_path_canon =
                    parent.join(APP_DEFAULT_PREDEF_CONFIG_FILE_CFG_RELATIVE);
            }
        }

        if !predef_cfg_file_path_canon.exists() {
            log::warn!(
                "Predefines config could not be found at {:?} or {:?}.",
                predef_cfg_file_path,
                predef_cfg_file_path_canon
            );
        }

        Ok(Self {
            cfg_file_path_canon,
            _predef_cfg_file_path: predef_cfg_file_path_canon.clone(),
            config: Config::default(),
            predefines: Self::load_predefines(&predef_cfg_file_path_canon)?,
            _config_changed: false,
            mappings: Vec::new(),
            resolved_midi_devices: BTreeMap::new(),
            resolved_mouse_devices: BTreeMap::new(),
            resolved_virtual_joysticks: BTreeMap::new(),
            debug,
        })
    }

    fn load_predefines(predef_cfg_file_path: &PathBuf) -> Result<ControlsPredefined> {
        if !predef_cfg_file_path.exists() {
            log::warn!(
                "Using empty predefines set. It's Ok if your main config doesn't use any \
                predefines. But if not... you'll see the errors below..."
            );
            return Ok(ControlsPredefined::default());
        }

        log::info!("Loading predefines from {}", predef_cfg_file_path.display());

        let contents =
            fs::read_to_string(predef_cfg_file_path).context("Failed to read predefines file")?;
        let raw_yaml: YamlValue =
            serde_yaml::from_str(&contents).context("Failed to parse predefines YAML as Value")?;
        let merged_keys_yaml = merge_keys_serde(raw_yaml)
            .map_err(|e| anyhow::anyhow!("Failed to apply YAML merges in predefines: {e}"))?;
        let controls: ControlsPredefined = serde_yaml::from_value(merged_keys_yaml)
            .context("Failed to deserialize predefines after merge expansion")?;
        Ok(controls)
    }

    // TODO: maybe use schema and serde deserialization for INCLUDE_YAML_KEY.
    fn resolve_includes(&self, val: YamlValue, base_path: &Path) -> Result<YamlValue> {
        match val {
            YamlValue::Mapping(mut map) => {
                let include_key: YamlValue = INCLUDE_YAML_KEY.into();
                if let Some(YamlValue::String(include_str)) = map.remove(&include_key) {
                    let included_yaml =
                        self.resolve_includes_get_yaml_from_path_query(&include_str, base_path)?;
                    if map.is_empty() {
                        return Ok(included_yaml);
                    }
                    let mut merged_map = serde_yaml::Mapping::new();
                    if let YamlValue::Mapping(inc_map) = included_yaml {
                        for (k, v) in inc_map {
                            merged_map.insert(k, v);
                        }
                    }
                    for (k, v) in map {
                        merged_map.insert(k, self.resolve_includes(v, base_path)?);
                    }
                    return Ok(YamlValue::Mapping(merged_map));
                }

                let mut new_map = serde_yaml::Mapping::new();
                for (k, v) in map {
                    new_map.insert(k, self.resolve_includes(v, base_path)?);
                }
                Ok(YamlValue::Mapping(new_map))
            }
            YamlValue::Sequence(seq) => {
                let mut new_seq = Vec::new();
                for item in seq {
                    new_seq.push(self.resolve_includes(item, base_path)?);
                }
                Ok(YamlValue::Sequence(new_seq))
            }
            _ => Ok(val),
        }
    }

    fn resolve_includes_get_yaml_from_path_query(
        &self,
        include_path_spec_str: &str,
        base_path: &Path,
    ) -> Result<YamlValue> {
        let mut parts: Vec<&str> = include_path_spec_str.split('/').collect();
        let mut file_path = base_path.to_owned();

        if !file_path.exists() {
            bail!("No file or dir exists at {}", base_path.display());
        }

        parts.reverse();
        while file_path.is_dir() {
            file_path = file_path.join(parts.pop().unwrap());
        }
        parts.reverse();

        if self.debug {
            log::debug!("Config include query chunks: {:?}", parts);
        }

        info!("Including config file {}", file_path.display());

        let content = fs::read_to_string(&file_path).context("Failed to read the included file")?;

        let root_val: YamlValue = serde_yaml::from_str(&content)
            .context("Failed to parse included file contents as YAML")?;

        if parts.is_empty() {
            if self.debug {
                log::debug!("Including config file: path query is empty, returning root node.");
            }
            return Ok(root_val);
        }

        let mut current_level_nodes: Vec<(YamlValue, YamlValue)> =
            vec![(YamlValue::Null, root_val)];

        for segment in &parts {
            let mut next_level_nodes = Vec::new();
            let re = Regex::new(&format!("^{}$", segment))?;

            for (_, node) in current_level_nodes {
                if let YamlValue::Mapping(map) = node {
                    for (k, v) in map {
                        if let YamlValue::String(key_str) = &k {
                            if re.is_match(key_str) {
                                next_level_nodes.push((k.clone(), v.clone()));
                            }
                        }
                    }
                }
            }
            current_level_nodes = next_level_nodes;
        }

        if current_level_nodes.is_empty() {
            log::warn!(
                "While including nodes from config {}: path query was not empty ({:?}), \
            but not nodes matched, returning empty result",
                file_path.display(),
                parts
            );
            return Ok(YamlValue::Mapping(serde_yaml::Mapping::new()));
        }

        let mut result_map = serde_yaml::Mapping::new();
        for (key, val) in current_level_nodes {
            result_map.insert(key, val);
        }

        Ok(YamlValue::Mapping(result_map))
    }

    pub(crate) fn load(&mut self) -> Result<()> {
        log::info!(
            "Loading user config from {}",
            self.cfg_file_path_canon.display()
        );

        // if !self.cfg_file_path.exists() {
        //     log::warn!(
        //         "Configuration file not found, creating default: {:?}",
        //         self.cfg_file_path
        //     );
        //     self.config = Config::default();
        //     self.config_changed = true;
        //     self.save()?;
        //     return Ok(());
        // }

        if !self.cfg_file_path_canon.exists() {
            log::error!("Config file is not found at {:?}", self.cfg_file_path_canon);
            bail!("Config file not found.");
        }

        let content =
            fs::read_to_string(&self.cfg_file_path_canon).context("Failed to read config file")?;

        let base_dir =
            fs::canonicalize(self.cfg_file_path_canon.parent().unwrap_or(Path::new(".")))
                .context("could not resolve main config base directory.")?;

        let raw_yaml: YamlValue =
            serde_yaml::from_str(&content).context("Initial YAML parse failed")?;

        let yaml_includes_resolved = self
            .resolve_includes(raw_yaml, base_dir.as_path())
            .context("Failed to resolve includes.")?;

        let yaml_keys_merged = merge_keys_serde(yaml_includes_resolved)
            .map_err(|e| anyhow::anyhow!("Failed merge YAML keys: {e}"))?;

        let final_yaml_str = serde_yaml::to_string(&yaml_keys_merged)?;
        match serde_yaml::from_str(&final_yaml_str) {
            Ok(merged_value) => self.config = merged_value,
            Err(e) => {
                let line = e.location().unwrap().line();
                log::error!("{e:?}");
                self.print_config_read_error_context(&final_yaml_str, line.saturating_sub(5), 7);
                bail!(e);
            }
        }

        self.resolve_all_devices()?;

        self.mappings = self.resolve_mappings()?;
        Ok(())
    }

    fn print_config_read_error_context(&mut self, src: &str, line0: usize, after: usize) {
        let start = line0;
        let end = line0.saturating_add(after);
        log::error!(" ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~ ");
        log::error!("... Context around erroneous config snippet: \n");
        for (i, line) in src.lines().enumerate() {
            if i < start {
                continue;
            } else if i > end {
                break;
            }
            log::error!("{:>6} | {}", i + 1, line);
        }
        log::error!(" ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~ ");
    }

    fn resolve_all_devices(&mut self) -> Result<()> {
        self.resolved_midi_devices.clear();
        self.resolved_mouse_devices.clear();
        self.resolved_virtual_joysticks.clear();

        if let Some(midi_devices) = &self.config.midi_devices {
            for (device_key, device) in midi_devices {
                let resolved = self
                    .resolve_midi_device(device)
                    .with_context(|| format!("Failed to resolve MIDI device '{}'", device_key))?;
                self.resolved_midi_devices
                    .insert(device_key.clone(), resolved);
            }
        }

        if let Some(mouse_devices) = &self.config.mouse_devices {
            for (device_key, device) in mouse_devices {
                let resolved = self
                    .resolve_mouse_device(device)
                    .with_context(|| format!("Failed to resolve mouse device '{}'", device_key))?;
                self.resolved_mouse_devices
                    .insert(device_key.clone(), resolved);
            }
        }

        for (joystick_key, joystick) in &self.config.virtual_joysticks {
            let resolved = self.resolve_virtual_joystick(joystick).with_context(|| {
                format!("Failed to resolve virtual joystick '{}'", joystick_key)
            })?;
            self.resolved_virtual_joysticks
                .insert(joystick_key.clone(), resolved);
        }

        Ok(())
    }

    fn resolve_mappings(&self) -> Result<Vec<ResolvedMapping>> {
        let mut resolved = Vec::new();
        for mapping in &self.config.mappings {
            if !mapping.enabled {
                continue;
            }
            let source = self.resolve_source(&mapping.source)?;
            let destination = self.resolve_destination(&mapping.destination)?;
            let transformation = self
                .resolve_transformation(&mapping.transformation)
                .with_context(|| {
                    format!(
                        "Failed to resolve transformation for mapping {:?}",
                        mapping.name
                    )
                })?;

            resolved.push(ResolvedMapping {
                enabled: mapping.enabled,
                name: mapping.name.clone(),
                source,
                destination,
                transformation,
                idle_tick_requirement_info__: Mutex::new(IdleTickRequirementInfo::new()),
            });
        }
        Ok(resolved)
    }

    fn resolve_transformation(
        &self,
        transformation: &Transformation,
    ) -> Result<ResolvedTransformation> {
        let mut resolved_steps = Vec::new();
        for step in transformation {
            resolved_steps.push(self.resolve_transformation_step(step)?);
        }
        Ok(resolved_steps)
    }

    fn resolve_transformation_step(
        &self,
        step: &TransformationStep,
    ) -> Result<ResolvedTransformationStep> {
        static CURRENT_STATE_ID: AtomicUsize = AtomicUsize::new(0);
        match step {
            TransformationStep::Steering { steering } => Ok(ResolvedTransformationStep::Steering {
                runtime_state_id: CURRENT_STATE_ID
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                steering: ResolvedSteeringTransform {
                    counts_to_lock: steering.counts_to_lock,
                    deadzone_counts: steering.deadzone_counts,
                    smoothing_alpha: steering.smoothing_alpha,
                    auto_center_halflife: steering.auto_center_halflife,
                    hold_factor: self.resolve_hold_factor(&steering.hold_factor)?,
                    force_feedback: steering.force_feedback.clone(),
                    user_input_power_curve: steering.user_input_power_curve.clone(),
                    user_input_ema_filter_runtime_state_id: CURRENT_STATE_ID
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    user_input_ema_filter_average: steering.user_input_ema_filter.clone(),
                },
            }),
            TransformationStep::PedalSmoother { pedal_smoother } => {
                Ok(ResolvedTransformationStep::PedalSmoother {
                    runtime_state_id: CURRENT_STATE_ID
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    pedal_smoother: ResolvedPedalSmootherTransform {
                        rise_rate: pedal_smoother.rise_rate,
                        fall_rate: pedal_smoother.fall_rate,
                        smoothing_alpha: pedal_smoother.smoothing_alpha,
                        fall_delay: pedal_smoother.fall_delay,
                        fall_gentling_factor: self
                            .resolve_hold_factor(&pedal_smoother.fall_gentling_factor)?,
                    },
                })
            }
            TransformationStep::Invert { invert } => Ok(ResolvedTransformationStep::Invert {
                invert: invert.clone(),
            }),
            TransformationStep::Integrate { integrate } => {
                Ok(ResolvedTransformationStep::Integrate {
                    runtime_state_id: CURRENT_STATE_ID
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    integrate: integrate.clone(),
                })
            }
            // TransformationStep::Curve { curve } => Ok(ResolvedTransformationStep::Curve {
            //     curve: curve.clone(),
            // }),
            TransformationStep::Clamp { clamp } => Ok(ResolvedTransformationStep::Clamp {
                clamp: clamp.clone(),
            }),
            TransformationStep::EmaFilter {
                ema_filter: moving_average,
            } => Ok(ResolvedTransformationStep::EmaFilter {
                runtime_state_id: CURRENT_STATE_ID
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                ema_filter: moving_average.clone(),
            }),
            TransformationStep::Linear { linear } => Ok(ResolvedTransformationStep::Linear {
                linear: linear.clone(),
            }),
            TransformationStep::Quadratic {
                quadratic_curve: quadratic,
            } => Ok(ResolvedTransformationStep::Quadratic {
                quadratic_curve: quadratic.clone(),
            }),
            TransformationStep::Cubic { cubic_curve: cubic } => {
                Ok(ResolvedTransformationStep::Cubic {
                    cubic_curve: cubic.clone(),
                })
            }
            TransformationStep::Smoothstep {
                smoothstep_curve: smoothstep,
            } => Ok(ResolvedTransformationStep::Smoothstep {
                smoothstep_curve: smoothstep.clone(),
            }),
            TransformationStep::SCurve { s_curve } => Ok(ResolvedTransformationStep::SCurve {
                s_curve: s_curve.clone(),
            }),
            TransformationStep::Exponential { exp_curve: exp } => {
                Ok(ResolvedTransformationStep::Exponential {
                    exp_curve: exp.clone(),
                })
            }
            TransformationStep::Power { power_curve: power } => {
                Ok(ResolvedTransformationStep::Power {
                    power_curve: power.clone(),
                })
            }
            TransformationStep::SymmetricPower {
                symmetric_power_curve: symmetric_power,
            } => Ok(ResolvedTransformationStep::SymmetricPower {
                symmetric_power_curve: symmetric_power.clone(),
            }),
            TransformationStep::LowPass { lowpass } => Ok(ResolvedTransformationStep::LowPass {
                runtime_state_id: CURRENT_STATE_ID
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                lowpass: lowpass.clone(),
            }),
            TransformationStep::_HighPass { highpass } => {
                Ok(ResolvedTransformationStep::HighPass {
                    runtime_state_id: CURRENT_STATE_ID
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    highpass: highpass.clone(),
                })
            }
        }
    }

    fn resolve_hold_factor(
        &self,
        factor: &Option<HoldFactor>,
    ) -> Result<Option<ResolvedHoldFactor>> {
        match factor {
            Some(HoldFactor::Value(v)) => Ok(Some(ResolvedHoldFactor::Value(*v))),
            Some(HoldFactor::Reference { device, control }) => {
                if let Some(range) = self.lookup_control_range(device, control) {
                    Ok(Some(ResolvedHoldFactor::Reference {
                        device: device.clone(),
                        control: control.clone(),
                        range,
                    }))
                } else {
                    bail!(
                        "Hold factor reference to {}/{} could not be resolved (device or control not found)",
                        device,
                        control
                    );
                }
            }
            None => Ok(None),
        }
    }

    fn expand_midi_control(&self, entry: &ControlEntry<MidiControl>) -> Result<MidiControl> {
        let (predefined_name, base_control) = match entry {
            ControlEntry::Shorthand(name) => (Some(name.as_str()), MidiControl::default()),
            ControlEntry::Full(ctrl) => (ctrl.merge_from.as_deref(), ctrl.clone()),
        };

        if let Some(predef_name) = predefined_name {
            if let Some(predef) = self.predefines.midi_controls.get(predef_name) {
                let mut expanded = base_control;
                // Fill in missing fields from predefined
                if expanded.midi_message.is_none() {
                    expanded.midi_message = Some(predef.midi_message.clone());
                }
                if expanded.range.is_none() {
                    expanded.range = Some(predef.range);
                }
                if expanded.description.is_none() {
                    expanded.description = Some(predef.description.clone());
                }
                return Ok(expanded);
            } else {
                bail!(
                    "Unknown predefined MIDI control '{}'. Available: {:?}",
                    predef_name,
                    self.predefines.midi_controls.keys().collect::<Vec<_>>()
                );
            }
        }

        if base_control.midi_message.is_none() {
            bail!(
                "MIDI control has neither 'midi_message' nor valid 'predefined_type': {:?}",
                entry
            );
        }
        Ok(base_control)
    }

    fn expand_mouse_control(
        &self,
        entry: &ControlEntry<MouseControl>,
    ) -> Result<ResolvedMouseControl> {
        let (predefined_name, base_control) = match entry {
            ControlEntry::Shorthand(name) => (Some(name.as_str()), MouseControl::default()),
            ControlEntry::Full(ctrl) => (ctrl.merge_from.as_deref(), ctrl.clone()),
        };

        let mut control_type = base_control.r#type;
        let mut range = base_control.range;
        let mut description = base_control.description;

        if let Some(predef_name) = predefined_name {
            if let Some(predef) = self.predefines.mouse_controls.get(predef_name) {
                if control_type.is_none() {
                    control_type = Some(predef.r#type);
                }
                if range.is_none() {
                    range = Some(predef.range);
                }
                if description.is_none() {
                    description = Some(predef.description.clone());
                }
            } else {
                bail!(
                    "Unknown predefined mouse control '{}'. Available: {:?}",
                    predef_name,
                    self.predefines.mouse_controls.keys().collect::<Vec<_>>()
                );
            }
        }

        let control_type = control_type.ok_or_else(|| {
            anyhow::anyhow!("Mouse control missing 'type' and no valid predefined_type")
        })?;
        let range = range.ok_or_else(|| {
            anyhow::anyhow!("Mouse control missing 'range' and no valid predefined_type")
        })?;

        Ok(ResolvedMouseControl {
            r#type: control_type,
            range,
            _description: description,
        })
    }

    fn expand_joystick_control(
        &self,
        entry: &ControlEntry<JoystickControl>,
    ) -> Result<ResolvedJoystickControl> {
        let (predefined_name, base_control) = match entry {
            ControlEntry::Shorthand(name) => (Some(name.as_str()), JoystickControl::default()),
            ControlEntry::Full(ctrl) => (ctrl.merge_from.as_deref(), ctrl.clone()),
        };

        let mut control_type = base_control.r#type;
        let mut range = base_control.range;
        let mut properties = base_control.properties;
        let initial_value = base_control.initial_value;

        if let Some(predef_name) = predefined_name {
            if let Some(predef) = self.predefines.joystick_controls.get(predef_name) {
                if control_type.is_none() {
                    control_type = Some(predef.r#type);
                }
                if range.is_none() {
                    range = Some(predef.range);
                }
                if properties.is_none() {
                    properties = predef.properties.clone();
                }
            } else {
                bail!(
                    "Unknown predefined joystick control '{}'. Available: {:?}",
                    predef_name,
                    self.predefines.joystick_controls.keys().collect::<Vec<_>>()
                );
            }
        }

        let control_type = control_type.ok_or_else(|| {
            anyhow::anyhow!("Joystick control missing 'type' and no valid predefined_type")
        })?;
        let range = range.ok_or_else(|| {
            anyhow::anyhow!("Joystick control missing 'range' and no valid predefined_type")
        })?;

        Ok(ResolvedJoystickControl {
            r#type: control_type,
            range,
            properties,
            initial_value,
            idle_tick_enabled_flag: base_control.idle_tick_enabled_flag,
        })
    }

    pub(crate) fn resolve_virtual_joystick(
        &self,
        joystick: &VirtualJoystick,
    ) -> Result<ResolvedVirtualJoystick> {
        let mut resolved_controls = HashMap::new();

        for (control_name, control_entry) in &joystick.controls {
            let resolved_control = self
                .expand_joystick_control(control_entry)
                .with_context(|| format!("Failed to expand joystick control '{}'", control_name))?;
            resolved_controls.insert(control_name.clone(), resolved_control);
        }

        Ok(ResolvedVirtualJoystick {
            enabled: joystick.enabled.unwrap_or(true),
            persistent: joystick
                .persistent
                .unwrap_or(self.config.global.persistent_joysticks),
            name: joystick.name.clone(),
            properties: joystick.properties.clone(),
            controls: resolved_controls,
            force_feedback: joystick.force_feedback.clone(),
        })
    }

    pub(crate) fn resolve_midi_device(&self, device: &MidiDevice) -> Result<ResolvedMidiDevice> {
        let mut resolved_controls = HashMap::new();

        for (control_name, control_entry) in &device.controls {
            let resolved_control = self
                .expand_midi_control(control_entry)
                .with_context(|| format!("Failed to expand MIDI control '{}'", control_name))?;
            resolved_controls.insert(control_name.clone(), resolved_control);
        }

        Ok(ResolvedMidiDevice {
            enabled: device.enabled,
            match_name_regex: device.match_name_regex.clone(),
            controls: resolved_controls,
        })
    }

    pub(crate) fn resolve_mouse_device(&self, device: &MouseDevice) -> Result<ResolvedMouseDevice> {
        let mut resolved_controls = HashMap::new();

        for (control_name, control_entry) in &device.controls {
            let resolved_control = self
                .expand_mouse_control(control_entry)
                .with_context(|| format!("Failed to expand mouse control '{}'", control_name))?;
            resolved_controls.insert(control_name.clone(), resolved_control);
        }

        Ok(ResolvedMouseDevice {
            enabled: device.enabled,
            match_name_regex: device.match_name_regex.clone(),
            controls: resolved_controls,
        })
    }

    fn resolve_source(&self, source: &MappingSource) -> Result<ResolvedMappingSource> {
        if let Some(resolved_device) = self.resolved_midi_devices.get(&source.device) {
            if let Some(resolved_control) = resolved_device.controls.get(&source.control) {
                return Ok(ResolvedMappingSource {
                    device_key: source.device.clone(),
                    control_key: source.control.clone(),
                    control: ControlReference::Midi(resolved_control.clone()),
                });
            }
        }

        if let Some(resolved_device) = self.resolved_mouse_devices.get(&source.device) {
            if let Some(resolved_control) = resolved_device.controls.get(&source.control) {
                return Ok(ResolvedMappingSource {
                    device_key: source.device.clone(),
                    control_key: source.control.clone(),
                    control: ControlReference::Mouse(resolved_control.clone()),
                });
            }
        }

        bail!(
            "Failed to resolve source device '{}' or control '{}'",
            source.device,
            source.control
        )
    }

    fn resolve_destination(&self, dest: &MappingDestination) -> Result<ResolvedMappingDestination> {
        if let Some(resolved_joystick) = self.resolved_virtual_joysticks.get(&dest.joystick) {
            if let Some(resolved_control) = resolved_joystick.controls.get(&dest.control) {
                let joystick = self
                    .config
                    .virtual_joysticks
                    .get(&dest.joystick)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Joystick '{}' not found in config", dest.joystick)
                    })?;

                return Ok(ResolvedMappingDestination {
                    device_key: dest.joystick.clone(),
                    joystick: joystick.clone(),
                    control_key: dest.control.clone(),
                    control: resolved_control.clone(),
                });
            }
        }

        bail!(
            "Failed to resolve destination joystick '{}' or control '{}'",
            dest.joystick,
            dest.control
        )
    }

    // -------------------------------------------------------
    pub(crate) fn _save(&mut self) -> Result<()> {
        self.config.last_modified = Some(Utc::now());

        let yaml = serde_yaml::to_string(&self.config)?;

        let header = format!(
            "# {APP_NAME} Configuration File\n\
             # Version: {CONFIG_VERSION_STR}\n\
             # Last modified: {}\n\n",
            self.config.last_modified.unwrap()
        );

        fs::write(&self.cfg_file_path_canon, header + &yaml)
            .context("Failed to write config file")?;

        self._config_changed = false;
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<Vec<String>> {
        let mut errors = Vec::new();

        for (i, mapping) in self.config.mappings.iter().enumerate() {
            let src_dev = &mapping.source.device;
            let src_ctrl = &mapping.source.control;
            let dst_joy = &mapping.destination.joystick;
            let dst_ctrl = &mapping.destination.control;

            let found_in_midi = self
                .config
                .midi_devices
                .as_ref()
                .is_some_and(|m| m.contains_key(src_dev));

            let found_in_mouse = self
                .config
                .mouse_devices
                .as_ref()
                .map(|m| m.contains_key(src_dev))
                .unwrap_or(false);

            if !found_in_midi && !found_in_mouse {
                errors.push(format!(
                    "Mapping[{}] references unknown device '{}'",
                    i, src_dev
                ));
            } else if found_in_midi {
                if let Some(midi_devices) = &self.config.midi_devices {
                    if let Some(device) = midi_devices.get(src_dev) {
                        if !device.controls.contains_key(src_ctrl) {
                            errors.push(format!(
                                "Mapping[{}] references unknown control '{}' in midi_devices['{}']",
                                i, src_ctrl, src_dev
                            ));
                        }
                    }
                }
            }

            if !self.config.virtual_joysticks.contains_key(dst_joy) {
                errors.push(format!(
                    "Mapping[{}] references unknown virtual joystick '{}'",
                    i, dst_joy
                ));
            } else if let Some(joystick) = self.config.virtual_joysticks.get(dst_joy) {
                if !joystick.controls.contains_key(dst_ctrl) {
                    errors.push(format!(
                        "Mapping[{}] references unknown control '{}' in virtual_joysticks['{}']",
                        i, dst_ctrl, dst_joy
                    ));
                }
            }
        }

        Ok(errors)
    }

    pub(crate) fn get_config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn get_mappings(&self) -> &[ResolvedMapping] {
        &self.mappings
    }

    pub(crate) fn get_resolved_virtual_joystick(
        &self,
        key: &str,
    ) -> Option<&crate::schemas::ResolvedVirtualJoystick> {
        self.resolved_virtual_joysticks.get(key)
    }

    pub(crate) fn get_resolved_virtual_joysticks(
        &self,
    ) -> &BTreeMap<String, crate::schemas::ResolvedVirtualJoystick> {
        &self.resolved_virtual_joysticks
    }

    pub(crate) fn get_resolved_midi_device(
        &self,
        key: &str,
    ) -> Option<&crate::schemas::ResolvedMidiDevice> {
        self.resolved_midi_devices.get(key)
    }

    pub(crate) fn get_resolved_mouse_device(
        &self,
        key: &str,
    ) -> Option<&crate::schemas::ResolvedMouseDevice> {
        self.resolved_mouse_devices.get(key)
    }

    fn lookup_control_range(
        &self,
        device_key: &str,
        control_key: &str,
    ) -> Option<NumInterval<i32>> {
        if let Some(resolved_device) = self.resolved_midi_devices.get(device_key) {
            if let Some(resolved_control) = resolved_device.controls.get(control_key) {
                return resolved_control.range;
            }
        }

        if let Some(resolved_device) = self.resolved_mouse_devices.get(device_key) {
            if let Some(resolved_control) = resolved_device.controls.get(control_key) {
                return Some(resolved_control.range);
            }
        }

        if let Some(resolved_joystick) = self.resolved_virtual_joysticks.get(device_key) {
            if let Some(resolved_control) = resolved_joystick.controls.get(control_key) {
                return Some(resolved_control.range);
            }
        }

        None
    }
}
