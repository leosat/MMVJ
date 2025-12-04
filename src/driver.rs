use anyhow::{bail, Result};
use clap::Subcommand;
use log::{error, info, warn};

use crate::config::ConfigManager;
use crate::joystick::VirtualJoystickManager;
use crate::mapping::MappingEngine;
use crate::midi::{MidiLearnMode, MidiManager};
use crate::mouse::MouseManager;

#[derive(Subcommand, Clone)]
pub enum AuxDriverTask {
    EnumMidi,
    MonitorMidi { name_regex: Option<String> },
    MidiLearn,
    EnumMice,
    MonitorMouse { name_regex: Option<String> },
    ValidateConfig,
}

pub async fn run_aux_task(
    aux_task: &AuxDriverTask,
    config_path: std::path::PathBuf,
    debug: bool,
) -> Result<()> {
    match aux_task {
        AuxDriverTask::EnumMidi => {
            info!("Available MIDI devices:");
            for (i, device) in MidiManager::new(debug)?
                .enumerate_devices()
                .iter()
                .enumerate()
            {
                info!("> {}. {}", i + 1, device.name);
            }
        }
        AuxDriverTask::MonitorMidi { name_regex: device } => {
            MidiManager::new(debug)?
                .monitor(&regex::Regex::new(
                    &device.clone().unwrap_or(".*".to_string()),
                )?)
                .await?;
        }
        AuxDriverTask::MidiLearn => {
            MidiLearnMode::new(MidiManager::new(debug)?).run().await?;
        }
        AuxDriverTask::EnumMice => {
            info!("Available mouse devices:");
            for device in MouseManager::new(debug)?.enumerate_devices()? {
                info!("> {} @ {}", device.name, device.path.display());
            }
        }
        AuxDriverTask::MonitorMouse { name_regex: device } => {
            MouseManager::new(debug)?
                .monitor(&regex::Regex::new(
                    &device.clone().unwrap_or(".*".to_string()),
                )?)
                .await?;
        }
        AuxDriverTask::ValidateConfig => {
            let mut config_manager = ConfigManager::new(&config_path)?;
            config_manager.load()?;
            let errors = config_manager.validate()?;
            if errors.is_empty() {
                info!("Configuration is valid.");
            } else {
                error!("Configuration errors:");
                for error in errors {
                    error!("> {}", error);
                }
                bail!("Configuration validation failed");
            }
        }
    }
    Ok(())
}

pub async fn run_mapping_engine(
    config_path: std::path::PathBuf,
    debug: bool,
    debug_ff: bool,
    debug_idle_tick: bool,
    update_rate_hz: u32,
) -> Result<()> {
    info!("Starting mapping engine.");
    let mut config_manager = ConfigManager::new(&config_path)?;
    config_manager.load()?;

    let mut engine = MappingEngine::new(
        &config_manager,
        MidiManager::new(debug)?,
        MouseManager::new(debug)?,
        VirtualJoystickManager::new(debug, debug_ff)?,
        debug,
        debug_idle_tick,
    )?;

    engine.set_update_rate(update_rate_hz);
    // engine.set_latency_mode(&latency_mode);
    engine.initialize().await?;

    {
        let active_mappings = engine.active_mapping_count();
        if active_mappings == 0 {
            warn!("No active mappings found - nothing to do");
            return Ok(());
        }
        info!("Active mappings: {}", active_mappings);
    }

    info!("Press Ctrl+C to stop");
    info!("{}", "=".repeat(50));

    tokio::select! {
        result = engine.run() => {
            result?;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received request to quit.");
            engine.stop()?;
            info!("Cleanup complete, terminating immediately.");
            std::process::exit(0);
        }
    }

    Ok(())
}

pub fn check_linux_system_requirements() -> Result<()> {
    if !std::path::Path::new("/dev/uinput").exists() {
        error!("/dev/uinput not found. Force feedback will not work.");
        error!("Run: sudo modprobe uinput");
    }

    if !nix::unistd::Uid::current().is_root() {
        let groups = nix::unistd::getgroups()?;
        let input_gid = nix::unistd::Group::from_name("input")?.map(|g| g.gid);

        if let Some(gid) = input_gid {
            if !groups.contains(&gid) {
                warn!("Warning: Current user not in 'input' group");
                warn!("Run: sudo usermod -a -G input $USER");
                warn!("Then logout and login again");
            }
        }
    }

    Ok(())
}
