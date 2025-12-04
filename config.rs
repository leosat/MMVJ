use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use yaml_merge_keys::merge_keys_serde;

use crate::common::Interval;
use crate::schemas::*;

pub(crate) const APP_VERSION_STR: &str = "3.14";
pub(crate) const CONFIG_VERSION_STR: &str = "2.0";
pub(crate) const CONFIG_PREDEFINES_FILE: &str = "mmvj_config_predefines.yaml";
pub(crate) const APP_AUTHORS: &str =
    "Leonid Satanovskiy and small furry creatures from Alpha Centauri.";
pub(crate) const APP_NAME: &str = "MMVJ";
pub(crate) const APP_LONG_NAME: &str = "Mouse and MIDI to Virtual Joystick (Transfor)Mapper";
pub(crate) const APP_ABOUT: &str = APP_LONG_NAME;
pub(crate) const APP_LONG_ABOUT: &str = APP_ABOUT;
pub(crate) const APP_DEFAULT_CONFIG_FILE: &str = "mmvj_cfg.yaml";
pub(crate) const APP_DEFAULT_LATENCY_STR: &str = "normal";
pub(crate) const APP_DEFAULT_UPDATE_FREQ_HZ_STR: &str = "1000";
pub(crate) const APP_DEFAULT_MAX_LOG_LEVEL: &str = "debug";

// pub(crate) const DEFAULT_CONFIG_FILE: &str = "mmvj_cfg.yaml";
// pub(crate) const AUTOLEARN_CONFIG_FILE: &str = "mmvj_autolearn.yaml";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) global: GlobalSettings,
    #[serde(default)]
    pub(crate) profiles: Option<Profiles>,
    #[serde(default)]
    pub(crate) presets: Option<Presets>,
    #[serde(default)]
    pub(crate) midi_devices: Option<HashMap<String, MidiDevice>>, // Changed to Option
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
            version: CONFIG_VERSION_STR.to_string(),
            global: GlobalSettings::default(),
            profiles: None,
            presets: None,
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
    // pub(crate) id: Option<String>,
    // pub(crate) name: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) source: ResolvedMappingSource,
    pub(crate) destination: ResolvedMappingDestination,
    pub(crate) transformation: Transformation,
    pub(crate) idle_tick_requirement_info__: Mutex<IdleTickRequirementInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedMappingSource {
    pub(crate) device_key: String,
    // pub(crate) device: DeviceReference,
    pub(crate) control_key: String,
    pub(crate) control: ControlReference,
}

// #[derive(Debug, Clone)]
// pub(crate) enum DeviceReference {
//     Midi(MidiDevice),
//     Mouse(MouseDevice),
// }

#[derive(Debug, Clone)]
pub(crate) enum ControlReference {
    Midi(MidiControl),
    Mouse(MouseControl),
}

#[derive(Debug)]
pub(crate) struct ResolvedMappingDestination {
    pub(crate) device_key: String,
    #[allow(dead_code)]
    pub(crate) joystick: VirtualJoystick,
    pub(crate) control_key: String,
    pub(crate) control: JoystickControl,
}

// >>>>----------------------------------------------
// >>>> Support ResolvedMapping as a hashmap key
// >>>>----------------------------------------------
impl PartialEq for ResolvedMappingSource {
    fn eq(&self, other: &Self) -> bool {
        // A source is defined only by its key strings
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
        // A destination is defined only by its key strings
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
        // The identity of a mapping is solely defined by its endpoints
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

// <<<<----------------------------------------------
// <<<< Support ResolvedMapping as a hashmap key
// <<<<----------------------------------------------

#[derive(Debug)]
pub(crate) struct ConfigManager {
    config_file: PathBuf,
    config: Config,
    predefines: InternalControls,
    config_changed: bool,
    mappings: Vec<ResolvedMapping>,
    control_range_cache: Mutex<HashMap<String, Option<Interval<i32>>>>,
}

impl ConfigManager {
    pub(crate) fn new(config_file: &Path) -> Result<Self> {
        Ok(Self {
            config_file: config_file.to_path_buf(),
            config: Config::default(),
            predefines: Self::load_predefines(&PathBuf::from(CONFIG_PREDEFINES_FILE))?,
            config_changed: false,
            mappings: Vec::new(),
            control_range_cache: Mutex::new(HashMap::new()),
        })
    }

    fn load_predefines(path: &Path) -> Result<InternalControls> {
        log::info!("Loading predefines from {}", path.display());
        if !path.exists() {
            log::warn!(
                "Predefines config {} could not be loaded, using empty predefines set.",
                path.display()
            );
            return Ok(InternalControls::default());
        }
        let contents = fs::read_to_string(path).context("Failed to read predefines file")?;
        // Expand YAML merge keys (<<) before deserializing
        let raw: YamlValue =
            serde_yaml::from_str(&contents).context("Failed to parse predefines YAML as Value")?;
        let merged = merge_keys_serde(raw)
            .map_err(|e| anyhow::anyhow!("Failed to apply YAML merges in predefines: {e}"))?;
        let controls: InternalControls = serde_yaml::from_value(merged)
            .context("Failed to deserialize predefines after merge expansion")?;
        Ok(controls)
    }

    pub(crate) fn load(&mut self) -> Result<()> {
        if let Ok(mut cache) = self.control_range_cache.lock() {
            cache.clear();
        }

        if !self.config_file.exists() {
            log::info!(
                "Configuration file not found, creating default: {:?}",
                self.config_file
            );
            self.config = Config::default();
            self.config_changed = true;
            self.save()?;
        } else {
            self.config = serde_yaml::from_value(
                merge_keys_serde(
                    serde_yaml::from_str(
                        &fs::read_to_string(&self.config_file)
                            .context("Failed to read config file")?,
                    )
                    .context("Failed to parse config YAML as Value")?,
                )
                .map_err(|e| anyhow::anyhow!("Failed to apply YAML merges in config: {e}"))?,
            )
            .context("Failed to deserialize config after merge expansion")?;
        }

        self.resolve_mappings()?;
        Ok(())
    }

    fn resolve_mappings(&mut self) -> Result<()> {
        let mut resolved = Vec::new();
        for mapping in &self.config.mappings {
            resolved.push(ResolvedMapping {
                // id: mapping.id.clone(),
                // name: mapping.name.clone(),
                enabled: mapping.enabled,
                source: self.resolve_source(&mapping.source)?,
                destination: self.resolve_destination(&mapping.destination)?,
                transformation: mapping.transformation.clone(),
                idle_tick_requirement_info__: Mutex::new(IdleTickRequirementInfo::new()),
            });
        }
        self.mappings = resolved;
        Ok(())
    }

    fn expand_midi_control(&self, control: &MidiControl) -> Result<MidiControl> {
        if control.midi_message.is_some() {
            return Ok(control.clone());
        }
        if let Some(predefined_type) = &control.predefined_type {
            if let Some(predefined_control) = self.predefines.midi_controls.get(predefined_type) {
                let mut expanded = control.clone();
                expanded.midi_message = Some(predefined_control.midi_message.clone());
                if expanded.range.is_none() {
                    expanded.range = predefined_control.range;
                }
                if expanded.description.is_none() {
                    expanded.description = Some(predefined_control.description.clone());
                }
                return Ok(expanded);
            } else {
                log::warn!(
                    "Unknown predefined type '{}'. Available types: {:?}",
                    predefined_type,
                    self.predefines.midi_controls.keys().collect::<Vec<_>>()
                );
                return Ok(control.clone());
            }
        }
        log::warn!(
            "Control has neither 'midi_message' nor 'predefined_type': {:?}",
            control
        );
        Ok(control.clone())
    }

    fn resolve_source(&self, source: &MappingSource) -> Result<ResolvedMappingSource> {
        if let Some(midi_devices) = &self.config.midi_devices {
            if let Some(midi_device) = midi_devices.get(&source.device) {
                if let Some(control) = midi_device.controls.get(&source.control) {
                    let expanded_control = match self.expand_midi_control(control) {
                        Ok(ctrl) => ctrl,
                        Err(e) => {
                            log::warn!(
                                "Failed to expand MIDI control '{}' in device '{}': {}. Using control as-is.",
                                source.control,
                                source.device,
                                e
                            );
                            control.clone()
                        }
                    };

                    return Ok(ResolvedMappingSource {
                        device_key: source.device.clone(),
                        // device: DeviceReference::Midi(midi_device.clone()),
                        control_key: source.control.clone(),
                        control: ControlReference::Midi(expanded_control),
                    });
                }
            }
        }

        if let Some(mouse_devices) = &self.config.mouse_devices {
            if let Some(mouse_device) = mouse_devices.get(&source.device) {
                if let Some(control) = mouse_device.controls.get(&source.control) {
                    return Ok(ResolvedMappingSource {
                        device_key: source.device.clone(),
                        // device: DeviceReference::Mouse(mouse_device.clone()),
                        control_key: source.control.clone(),
                        control: ControlReference::Mouse(control.clone()),
                    });
                }
            }
        }

        bail!(
            "Failed to resolve source device '{}' or control '{}'",
            source.device,
            source.control
        )
    }

    fn resolve_destination(&self, dest: &MappingDestination) -> Result<ResolvedMappingDestination> {
        if let Some(joystick) = self.config.virtual_joysticks.get(&dest.joystick) {
            if let Some(control) = joystick.controls.get(&dest.control) {
                return Ok(ResolvedMappingDestination {
                    device_key: dest.joystick.clone(),
                    joystick: joystick.clone(),
                    control_key: dest.control.clone(),
                    control: control.clone(),
                });
            }
        }

        bail!(
            "Failed to resolve destination joystick '{}' or control '{}'",
            dest.joystick,
            dest.control
        )
    }

    pub(crate) fn save(&mut self) -> Result<()> {
        self.config.last_modified = Some(Utc::now());

        let yaml = serde_yaml::to_string(&self.config)?;

        let header = format!(
            "# {APP_NAME} Configuration File\n\
             # Version: {CONFIG_VERSION_STR}\n\
             # Last modified: {}\n\n",
            self.config.last_modified.unwrap()
        );

        fs::write(&self.config_file, header + &yaml).context("Failed to write config file")?;

        self.config_changed = false;
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
                .map_or(false, |m| m.contains_key(src_dev));

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

    pub(crate) fn get_control_range(
        &self,
        device_or_joystick: &str,
        control: &str,
    ) -> Option<Interval<i32>> {
        let cache_key = format!("{}/{}", device_or_joystick, control);
        if let Ok(cache) = self.control_range_cache.lock() {
            if let Some(cached_result) = cache.get(&cache_key) {
                return *cached_result;
            }
        }
        let range = self.lookup_control_range(device_or_joystick, control);
        if let Ok(mut cache) = self.control_range_cache.lock() {
            cache.insert(cache_key, range);
        }
        range
    }

    fn lookup_control_range(
        &self,
        device_or_joystick: &str,
        control: &str,
    ) -> Option<Interval<i32>> {
        if let Some(midi_devices) = &self.config.midi_devices {
            if let Some(device) = midi_devices.get(device_or_joystick) {
                if let Some(ctrl) = device.controls.get(control) {
                    return ctrl.range;
                }
            }
        }
        if let Some(mouse_devices) = &self.config.mouse_devices {
            if let Some(device) = mouse_devices.get(device_or_joystick) {
                if let Some(ctrl) = device.controls.get(control) {
                    return Some(ctrl.range);
                }
            }
        }
        if let Some(joystick) = self.config.virtual_joysticks.get(device_or_joystick) {
            if let Some(ctrl) = joystick.controls.get(control) {
                return Some(ctrl.range);
            }
        }
        None
    }
}
