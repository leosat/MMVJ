use std::path::Path;

use anyhow::{bail, Result};
use clap::Subcommand;
use colored::Colorize;
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

fn sanitize_cfg_file_path(cfg_file_path: &std::path::Path) -> Result<()> {
    if !cfg_file_path.exists() {
        let e = format!(
            "Configuration file is not found at {}, current dir is {}.\n\
            Please specify proper location of the configuration file via -c command line option.\n\
            ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
            cfg_file_path.to_str().unwrap_or("Empty file path..."),
            std::env::current_dir()?
                .to_str()
                .unwrap_or("Unknown curent working dir...")
        );
        log::error!("{e}");

        if cfg_file_path.is_relative() {
            let nb = "NB: You have specified relative config file path. \n\
              NB: if running an appimage, the program gets running in a temporary directory \n\
             NB: for the config to be found specify full config path like -c <full path to your config>\n\
             ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~".to_string();
            log::warn!("{nb}");
        }
        bail!("Config file not found.");
    }
    Ok(())
}

pub async fn run_aux_task(
    aux_task: &AuxDriverTask,
    cfg_file_path: &Path,
    predef_cfg_file_path: &Path,
    debug: bool,
) -> Result<()> {
    sanitize_cfg_file_path(cfg_file_path)?;

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
            let mut config_manager =
                ConfigManager::new(cfg_file_path, predef_cfg_file_path, debug)?;
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

fn watch_config_file(cfg_file_path: &std::path::Path) -> Result<tokio::sync::mpsc::Receiver<()>> {
    use notify_debouncer_full::{new_debouncer, DebounceEventResult};

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut debouncer = new_debouncer(
        std::time::Duration::from_millis(500),
        None,
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                for event in events {
                    if event.kind.is_modify() || event.kind.is_create() {
                        let _ = tx.blocking_send(());
                        break;
                    }
                }
            }
            Err(e) => error!("Config file watch error: {:?}", e),
        },
    )?;

    debouncer.watch(
        cfg_file_path,
        notify_debouncer_full::notify::RecursiveMode::NonRecursive,
    )?;

    std::mem::forget(debouncer);

    Ok(rx)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_mapping_engine(
    cfg_file_path: &Path,
    predef_cfg_file_path: &Path,
    no_hot_reload: bool,
    debug: bool,
    debug_ff: bool,
    debug_idle_tick: bool,
    update_rate_hz: Option<u32>,
    persistent_joysticks_cli: Option<Vec<String>>,
    enable_steering_indicator_window: bool,
) -> Result<()> {
    sanitize_cfg_file_path(cfg_file_path)?;

    let mut config_watcher = if !no_hot_reload {
        Some(watch_config_file(cfg_file_path)?)
    } else {
        None
    };

    let mut config_manager = ConfigManager::new(cfg_file_path, predef_cfg_file_path, debug)?;
    let joystick_manager = VirtualJoystickManager::new(debug, debug_ff)?;
    let mut is_first_run = true;

    'engine_restart: loop {
        if debug {
            log::debug!("Loading configuration.");
        }

        config_manager.load()?;

        '_Persistent_Joysticks_Handling: {
            if !is_first_run {
                joystick_manager.stop(false)?;
            }
            for (key, resolved_joystick) in config_manager.get_resolved_virtual_joysticks() {
                if !resolved_joystick.enabled {
                    joystick_manager.destroy_virtual_joystick_if_exists(key);
                    continue;
                } else {
                    let mut is_persistent = resolved_joystick.persistent;
                    if let Some(cli_list) = &persistent_joysticks_cli {
                        if cli_list.contains(&"all".to_string()) || cli_list.contains(key) {
                            is_persistent = true;
                        }
                    }
                    joystick_manager.create_virtual_joystick(
                        key,
                        resolved_joystick,
                        is_persistent,
                    )?;
                }
            }
        }

        let mut engine = MappingEngine::new(
            &config_manager,
            MidiManager::new(debug)?,
            MouseManager::new(debug)?,
            &joystick_manager,
            debug,
            debug_idle_tick,
            enable_steering_indicator_window,
        )?;

        if debug {
            log::debug!("Configuring mapping engine.");
        }

        if let Some(rate) = update_rate_hz {
            engine.set_update_rate(rate);
        } else {
            engine.set_update_rate(config_manager.get_config().global.idle_tick_update_rate);
        }

        if debug {
            log::debug!("Initializing mapping engine.");
        }
        engine.initialize().await?;

        {
            let active_mappings = engine.active_mapping_count();
            if active_mappings == 0 {
                warn!("----");
                warn!("No active mappings found - nothing to do, spinning in vain (joysticks if configured are still there).");
                warn!("Please configure mappings in configuration file and we'll catch up with hot-reload.");
                warn!("You don't need to manually restart.");
                warn!("----");
            }
            info!("Active mappings: {}", active_mappings);
        }

        info!("Starting mapping engine.");

        if !no_hot_reload {
            info!(
                "{} {}.",
                "Hot-reload on configuration file change is active"
                    .magenta()
                    .bold(),
                "(disable with --no-hot-reload)"
            );
        }

        info!("{}", "Press Ctrl+C to stop.".green().bold());
        info!("{}", "=".repeat(50));

        is_first_run = false;

        'current_run: loop {
            #[rustfmt::skip]
            tokio::select! {
                result = engine.run() => { result? }
                _ = async {
                    match config_watcher.as_mut() {
                        Some(rx) => { let _ = rx.recv().await; }
                        None => std::future::pending().await,
                    };
                } => {
                    {
                        let config_check_result = ConfigManager::new(cfg_file_path, predef_cfg_file_path, debug)
                            .expect("Config file path validity should have already been checked on start and couldn't change since then \
                                    (somewhere around line 8675309). I want to believe!")
                            .load();

                        if config_check_result.is_err() {
                            log::error!("\n---\n!!! Configuration load failed while trying to hot-reload.");
                            log::error!("!!! Will continue running with previous config.    _o_O-`  \n---\n");
                            log::error!("The error was: \n {:?} \n", config_check_result);
                            log::warn!("Running with previous (valid) configuration.");
                            continue 'current_run;
                        }
                    }

                    info!("Configuration validated. Stopping mapping engine to restart with new configuration.");
                    engine.stop()?; // This stops midi/mouse/engine-loop, joystick stop handled at top of loop
                    info!("Restarting mapping engine with new configuration.");
                    continue 'engine_restart;
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Ctrl+C received, going to terminate. Stopping mapping engine.");
                    engine.stop()?;
                    // Full shutdown of joysticks on exit
                    joystick_manager.stop(true)?;
                    info!("Cleanup complete, terminating immediately.");
                    return Ok(())
                }
            }
        }
    }
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
