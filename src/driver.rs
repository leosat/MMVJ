use crate::config::ConfigManager;
use crate::config::MORE_DEBUG;
use crate::debug::DebugLevel;
use crate::hid_manager::{HidManager, WithDeviceClassification};
use crate::mapped_device::MappedDeviceManager;
use crate::mapping::MappingEngine;
use crate::mapping::MappingEngineCmd;
#[cfg(feature = "midi")]
use crate::midi::{MidiLearnMode, MidiManager};
use crate::schemas_cfg::Config;
use crate::schemas_mapping::Mapping;
use anyhow::{Context, Result, bail};
use clap::Subcommand;
use colored::Colorize;
use log::{error, info, warn};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
#[cfg(feature = "gui")]
use tokio_util::sync::CancellationToken;

const COMMAND_RECV_CAPACITY: usize = 100;

#[derive(Debug, Clone)]
pub(crate) enum DriverCmd {
    ChangeConfigSimple {
        cfg: Config,
    },
    ChangeVirtualHids {
        cfg: Config,
        restart_persistent: bool,
        report_done_tx: std::sync::mpsc::Sender<()>,
    },
    ChangeMappings {
        mappings: Option<Vec<Mapping>>,
        action: MappingEngineCmd,
    },
    ChangeIdleTickRate {
        rate: u32,
    },
    #[cfg(feature = "gui")]
    StatusGuiClosed,
    #[allow(unused)]
    SaveCfg {
        cfg_file: Option<PathBuf>,
        cfg_suffix: Option<String>,
    },
    LoadCfg {
        cfg_file: PathBuf,
        resp_tx: std::sync::mpsc::Sender<Result<Config, String>>,
    },
    Reload,
    #[allow(unused)]
    ReloadWithInitialCfg,
    #[allow(unused)]
    Halt,
}

impl PartialEq for DriverCmd {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

enum DriverResponseOneShotChannels {
    Empty(std::sync::mpsc::Sender<()>),
    Config(std::sync::mpsc::Sender<Result<Config, String>>),
}

#[derive(Subcommand, Clone)]
pub enum AuxDriverTask {
    #[cfg(feature = "midi")]
    EnumMidi,
    #[cfg(feature = "midi")]
    MonitorMidi {
        name_regex: Option<String>,
    },
    #[cfg(feature = "midi")]
    MidiLearn,
    EnumHid,
    MonitorHid {
        name_regex: Option<String>,
    },
    ValidateConfig {
        save_after_validation: Option<bool>,
    },
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
             ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~"
                .to_string();
            log::warn!("{nb}");
        }
        bail!("Config file not found.");
    }
    Ok(())
}

pub async fn run_aux_task(aux_task: &AuxDriverTask, cfg_file_path: &Path, debug: DebugLevel) -> Result<()> {
    sanitize_cfg_file_path(cfg_file_path)?;

    match aux_task {
        #[cfg(feature = "midi")]
        AuxDriverTask::EnumMidi => {
            info!("\n-------------------------\nAvailable MIDI devices:\n-------------------------");
            for device in MidiManager::new(debug)?.enumerate_available_devices() {
                info!("Name: {}", device.name);
            }
        }
        AuxDriverTask::EnumHid => {
            info!(
                "\n-------------------------\nAvailable HID (Mice/Keyboards/etc) devices:\n-------------------------"
            );
            for device in HidManager::new(debug, debug.is_on())?.enumerate_available_devices(None) {
                info!(
                    " Name: {} @@ Path: {} @@ Classification: {}",
                    device.name,
                    device.path.display(),
                    device.classification
                );
            }
        }
        #[cfg(feature = "midi")]
        AuxDriverTask::MonitorMidi { name_regex: device } => {
            MidiManager::new(debug)?
                .monitor(&regex::Regex::new(&device.clone().unwrap_or(".*".to_string()))?)
                .await?;
        }
        AuxDriverTask::MonitorHid { name_regex: device } => {
            HidManager::new(debug, debug.is_on())?
                .monitor(&regex::Regex::new(&device.clone().unwrap_or(".*".to_string()))?, None)
                .await?;
        }
        #[cfg(feature = "midi")]
        AuxDriverTask::MidiLearn => {
            MidiLearnMode::new(MidiManager::new(debug)?).run().await?;
        }
        AuxDriverTask::ValidateConfig {
            save_after_validation: save_after_valiadation,
        } => {
            let mut cfg_mgr = ConfigManager::new(cfg_file_path, debug)?;
            cfg_mgr.load()?;
            if save_after_valiadation.unwrap_or_default() {
                cfg_mgr.save(&None, &None)?;
            }
        }
    }
    Ok(())
}

fn watch_config_file(cfg_file_path: &std::path::Path, tx: tokio::sync::mpsc::Sender<()>) -> Result<()> {
    use notify_debouncer_full::{DebounceEventResult, new_debouncer};

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

    Ok(())
}

fn check_and_load_new_cfg(cfg_mgr: &mut ConfigManager, new_cfg_file: &Path, debug: DebugLevel) -> Result<()> {
    if let Err(e) = ConfigManager::new(new_cfg_file, debug)
        .context("Config file not found (didn't exist or was lost in space-time transition!)")?
        .load()
    {
        log::error!("\n---\n!!! Configuration load failed while trying to hot-reload.");
        log::error!("!!! Will continue running with previous config.    _o_O-`  \n---\n");
        log::error!("The error was: \n {:?} \n", e);
        log::warn!("Running with previous (valid) configuration.");
        bail!(e);
    } else {
        info!("Configuration validated. Stopping mapping engine to restart with new configuration.");
        cfg_mgr.set_cfg_file(new_cfg_file)?;
        cfg_mgr.load()?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    cfg_file_path: &Path,
    no_hot_reload: bool,
    debug: DebugLevel,
    debug_ff: bool,
    debug_idle_tick: bool,
    update_rate_hz: Option<u32>,
    persistent_joysticks_cli: Option<Vec<String>>,
    #[cfg(feature = "gui")] gui_monitors: bool,
    #[cfg(feature = "gui")] gui_full: bool,
) -> Result<()> {
    crate::debug::set_debug_level__(debug);

    sanitize_cfg_file_path(cfg_file_path)?;

    #[cfg(feature = "gui")]
    let any_gui = gui_monitors || gui_full;

    let mut post_restart_response_channel: Option<DriverResponseOneShotChannels> = None;

    #[cfg(feature = "gui")]
    let gui_monitor_only = gui_monitors && !gui_full;

    let (cfg_watcher_tx, mut cfg_watcher_rx) = tokio::sync::mpsc::channel::<()>(1);
    watch_config_file(cfg_file_path, cfg_watcher_tx)?;

    let mut cfg_mgr = ConfigManager::new(cfg_file_path, debug)?;
    let hid_mgr = HidManager::new(debug, debug_ff)?;
    let mut is_first_run = true;

    //----------------------------- COMMAND BUFFERS ----------------------------------
    let mut cmd_rx_buf: Vec<DriverCmd> = Vec::with_capacity(COMMAND_RECV_CAPACITY);
    let (cmd_channel_tx, mut cmd_channel_rx) = tokio::sync::mpsc::unbounded_channel::<DriverCmd>();

    //------------------------------ GUI---------------------------------
    #[cfg(feature = "gui")]
    let gui_thread_cancellation_token = CancellationToken::new();

    cfg_mgr.load()?;

    #[cfg(feature = "gui")]
    let mut gui_thread_handle = if any_gui {
        let command_channel_tx = cmd_channel_tx.clone();
        let cancellation_token = gui_thread_cancellation_token.clone();
        let cfg = cfg_mgr.cfg_ref().clone();
        Some(std::thread::spawn(move || {
            crate::gui_main::run(gui_monitor_only, command_channel_tx, cancellation_token, cfg)
        }))
    } else {
        None
    };

    'restart_mapping_engine: loop {
        // ------------------
        // shared_atomic_state.clear();
        // ------------------

        /* Virtual Joysticks */
        {
            if !is_first_run {
                hid_mgr.stop(false)?;
            }

            for (key, resolved_joystick) in cfg_mgr
                .cfg_ref()
                .devices
                .hid
                .iter()
                .filter(|d| d.1.get_classification().is_a_virtual())
            {
                if !resolved_joystick.is_enabled() {
                    hid_mgr.destroy_virtual_device_if_exists(key);
                    continue;
                } else {
                    let mut is_persistent = resolved_joystick.is_persistent();
                    if let Some(cli_list) = &persistent_joysticks_cli
                        && (cli_list.contains(&"all".to_string()) || cli_list.contains(&key.into()))
                    {
                        is_persistent = true;
                    }
                    log::info!("Creating virtual joystick {key}");
                    hid_mgr.create_virtual_device(key, resolved_joystick, is_persistent)?;
                }
            }
        }

        let mut mapping_engine = MappingEngine::new(
            debug,
            debug_idle_tick,
            // TODO: perf: maybe use shared memory and left-right pattern
            cfg_mgr.cfg_ref().clone(),
            &hid_mgr,
            #[cfg(feature = "midi")]
            MidiManager::new(debug)?,
        )?;

        if debug.is_on() {
            log::debug!("Configuring mapping engine.");
        }

        if let Some(rate) = update_rate_hz {
            mapping_engine.set_idle_tick_rate(rate);
        } else {
            mapping_engine.set_idle_tick_rate(cfg_mgr.cfg_ref().global.idle_tick_rate);
        }

        if debug.is_on() {
            log::debug!("Initializing mapping engine.");
            let _ = fs::write("cfg_tree.debug_dump.1.txt", format!("{:#?}", cfg_mgr.cfg_ref()));
        }

        if let Some(tx) = post_restart_response_channel {
            match tx {
                DriverResponseOneShotChannels::Empty(tx) => {
                    let _ = tx.send(()).inspect_err(|e| log::error!("{e}"));
                }
                DriverResponseOneShotChannels::Config(tx) => {
                    let _ = tx
                        .send(Ok(cfg_mgr.cfg_ref().clone()))
                        .inspect_err(|e| log::error!("{e}"));
                }
            }
            post_restart_response_channel = None;
        }

        mapping_engine.init()?;

        if debug.is_on() {
            let _ = fs::write("cfg_tree.debug_dump.2.txt", format!("{:#?}", cfg_mgr.cfg_ref()));
        }

        {
            let active_mappings = mapping_engine.active_mappings_count();
            if active_mappings == 0 {
                warn!("----");
                warn!(
                    "No active mappings found - nothing to do, spinning in vain (joysticks if configured are still there)."
                );
                warn!("Please configure mappings in configuration file and we'll catch up with hot-reload.");
                warn!("You don't need to manually restart.");
                warn!("----");
            }
            info!("Active mappings: {}", active_mappings);
        }

        info!("Starting...");

        #[cfg(feature = "gui")]
        if !any_gui {
            info!(
                "\n{}. \n  (Use --log-to-console for log output to console even in Gui mode \
                \n    Use --gui-monitor to run monitoring overlays only).",
                "Running in command line mode. To run with Gui, use --gui option."
                    .cyan()
                    .bold(),
            );
        }

        if !no_hot_reload {
            info!(
                "{} {}.",
                "Hot-reload on configuration file change is active".magenta().bold(),
                "(disable with --no-hot-reload)"
            );
        }

        info!("{}", "Press Ctrl+C to stop.".green().bold());
        info!("{}", "=".repeat(50));

        is_first_run = false;

        info!(
            "Mapping engine running at {} Hz ... ",
            mapping_engine.get_idle_tick_rate()
        );

        loop {
            #[rustfmt::skip]
            tokio::select! {
                // -------------------- COMMANDS --------------------------
                commands_rx_count = cmd_channel_rx.recv_many(&mut cmd_rx_buf, COMMAND_RECV_CAPACITY)
                  , if crate::config::GUI_ENABLED_CONST
                    && !cmd_channel_rx.is_closed()
                        => {
                    match crate::driver::handle_cmd(
                        //----
                        &mut cfg_mgr,
                        &hid_mgr,
                        //----
                        &mut mapping_engine,
                        //----
                        commands_rx_count,
                        &mut cmd_rx_buf,
                        debug
                    ) {
                        (DriverMainLoopAction::Continue,_) => {},
                        (DriverMainLoopAction::Halt,_) => {
                            #[cfg(feature = "gui")]
                            #[allow(clippy::option_map_unit_fn)]
                            if let Some(gui_thread_handle) = gui_thread_handle {
                                gui_thread_handle.join().ok().map(|v|
                                    log::info!("Gui thread finitied with status {v:?}, \
                                        proceeding to stop engine and exit the main process"));
                            }
                            hid_mgr.stop(true)?;
                            mapping_engine.stop()?;
                            std::process::exit(0);
                        }
                        (DriverMainLoopAction::GoToStartWithCurrentCfg, report_done_tx) => {
                            post_restart_response_channel = report_done_tx;
                            continue 'restart_mapping_engine;
                        }
                        (DriverMainLoopAction::GoToStartWithInitialCfg,_) => {
                            log::info!("Full reload with initial config {}", cfg_file_path.to_string_lossy());
                            mapping_engine.stop()?;
                            cfg_mgr.set_cfg_file(cfg_file_path).expect("Failed to set config file");
                            cfg_mgr.load().expect("Failed to load config");
                            cfg_mgr = ConfigManager::new(cfg_file_path, debug)?;
                            continue 'restart_mapping_engine
                        },
                    }
                }
                // ------------------ MAPPING ENGINE --------------------------
                _ = mapping_engine.run() => { }

                // ------------------ CONFIG WATCHER --------------------------
                _ = cfg_watcher_rx.recv(), if !no_hot_reload && (|| {
                        #[cfg(feature = "gui")]             // Config watcher will only trigger
                        return gui_thread_handle.is_none() || gui_monitor_only; // reload in case Gui is closed or monitors only Gui mode.
                        #[cfg(not(feature = "gui"))]
                        return true; })()
                => {
                    log::info!("Config watcher triggered, reloading with new configuration.");
                    let cfg_file_name = cfg_mgr.get_cfg_file();
                    if check_and_load_new_cfg(&mut cfg_mgr, &cfg_file_name, debug).is_ok() {
                        mapping_engine.stop()?;
                        continue 'restart_mapping_engine;
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Ctrl+C received, going to terminate. Stopping mapping engine.");
                    #[cfg(feature = "gui")]
                    gui_thread_cancellation_token.cancel();
                    mapping_engine.stop()?;
                    hid_mgr.stop(true)?;
                    #[cfg(feature = "gui")]
                    if let Some(gui_thread_handle) = gui_thread_handle.take() &&
                            !gui_thread_handle.is_finished() {
                        info!("Gui thread still not finished, terminating immediately.");
                        std::process::exit(0);
                    }
                    info!("Cleanup complete, terminating.");
                    return Ok(())
                }
            }
        }
    }
}

#[derive(Debug)]
enum DriverMainLoopAction {
    Continue,
    GoToStartWithCurrentCfg,
    GoToStartWithInitialCfg,
    Halt,
}

fn handle_cmd(
    cfg_mgr: &mut ConfigManager,
    hid_mgr: &HidManager,
    mapping_engine: &mut MappingEngine,
    cmd_rx_count: usize,
    cmd_rx_buf: &mut Vec<DriverCmd>,
    debug: DebugLevel,
) -> (DriverMainLoopAction, Option<DriverResponseOneShotChannels>) {
    let mut main_loop_action = DriverMainLoopAction::Continue;
    let mut post_reload_report_back_tx = None;
    if cmd_rx_count > 0 {
        cmd_rx_buf.dedup_by(|next, prev| {
            if let (DriverCmd::ChangeVirtualHids { .. }, DriverCmd::ChangeVirtualHids { .. }) = (&*next, &*prev) {
                std::mem::swap(next, prev);
                true
            } else if let (DriverCmd::ChangeMappings { .. }, DriverCmd::ChangeMappings { .. }) = (&*next, &*prev) {
                std::mem::swap(next, prev);
                true
            } else {
                false
            }
        });

        for cmd in cmd_rx_buf.drain(..) {
            match cmd {
                DriverCmd::ChangeMappings { mappings, action } => {
                    if let Some(mappings) = mappings {
                        cfg_mgr.cfg_mut().mappings = mappings.to_vec();
                        mapping_engine.set_mappings(&cfg_mgr.cfg_ref().mappings);
                    }

                    match action {
                        MappingEngineCmd::_None => {
                            log::debug!("Mapping update: simple");
                        }
                        MappingEngineCmd::UpdateMappingRouterIdleTickOnly => {
                            log::debug!("Mapping update: update idle tick info");
                            mapping_engine.idle_tick_mappings_reset();
                        }
                        MappingEngineCmd::UpdateMappingRouter => {
                            log::debug!("Mapping update: update router info");
                            let _ = mapping_engine.init();
                        }
                    }
                }
                #[cfg(feature = "gui")]
                DriverCmd::StatusGuiClosed => {
                    for mapping in cfg_mgr.cfg_mut().mappings.iter_mut() {
                        mapping.transformation.disable_gui_tracing();
                    }
                    // command_channel_rx.close(); // NB: the channel would be closed anyways after last sender is out, but...
                }
                DriverCmd::LoadCfg { ref cfg_file, resp_tx } => {
                    let loading_new_cfg_file = cfg_mgr.get_cfg_file() != *cfg_file;
                    match check_and_load_new_cfg(cfg_mgr, cfg_file, debug) {
                        Ok(_) => {
                            log::info!("Sending new config to Gui");
                            post_reload_report_back_tx = Some(DriverResponseOneShotChannels::Config(resp_tx));

                            if loading_new_cfg_file {
                                log::warn!("Loading new configuration file will make persistent joysticks stopped.");
                                hid_mgr.stop(true).expect("Stopping joysticks failed.");
                            }

                            let _ = mapping_engine.stop().inspect_err(|e| log::error!("{e}"));
                            main_loop_action = DriverMainLoopAction::GoToStartWithCurrentCfg;
                        }
                        Err(e) => {
                            let _ = resp_tx.send(Err(e.to_string())).inspect_err(|e| log::error!("{e}"));
                        }
                    }
                }
                DriverCmd::SaveCfg {
                    ref cfg_file,
                    ref cfg_suffix,
                } => {
                    if let Err(e) = cfg_mgr.save(cfg_file, cfg_suffix) {
                        log::error!("{e}");
                    } else {
                        log::info!("Config saved successfully.");
                    }
                }
                DriverCmd::ChangeIdleTickRate { rate } => {
                    cfg_mgr.cfg_mut().global.idle_tick_rate = rate;
                    mapping_engine.set_idle_tick_rate(rate);
                }
                DriverCmd::ChangeVirtualHids {
                    cfg,
                    restart_persistent,
                    report_done_tx,
                } => {
                    log::info!("Updating virtual joysticks definitions.");
                    let _ = mapping_engine.stop().inspect_err(|e| log::error!("{e}"));
                    cfg_mgr.set_cfg(cfg.clone());
                    mapping_engine.set_cfg(cfg);
                    if restart_persistent {
                        log::warn!("Virtual device persistence settings will be ignored due to changes requested.");
                        hid_mgr.stop(true).expect("Stopping HID devices failed.");
                    }
                    post_reload_report_back_tx = Some(DriverResponseOneShotChannels::Empty(report_done_tx));
                    main_loop_action = DriverMainLoopAction::GoToStartWithCurrentCfg;
                }
                DriverCmd::ChangeConfigSimple { cfg } => {
                    cfg_mgr.set_cfg(cfg.clone());
                    mapping_engine.set_cfg(cfg);
                }
                DriverCmd::ReloadWithInitialCfg => main_loop_action = DriverMainLoopAction::GoToStartWithInitialCfg,
                DriverCmd::Reload => {
                    main_loop_action = DriverMainLoopAction::GoToStartWithCurrentCfg;
                }
                DriverCmd::Halt => {
                    log::info!("Terminating due to halt command received, see ya!");
                    main_loop_action = DriverMainLoopAction::Halt;
                }
            }
        }
    }

    if MORE_DEBUG {
        dbg!(&main_loop_action);
    }

    (main_loop_action, post_reload_report_back_tx)
}

pub fn check_linux_system_requirements() -> Result<()> {
    if !std::path::Path::new("/dev/uinput").exists() {
        error!("/dev/uinput not found. Force feedback will not work.");
        error!("Run: sudo modprobe uinput");
    }

    if !nix::unistd::Uid::current().is_root() {
        let groups = nix::unistd::getgroups()?;
        let input_gid = nix::unistd::Group::from_name("input")?.map(|g| g.gid);

        if let Some(gid) = input_gid
            && !groups.contains(&gid)
        {
            warn!("Warning: Current user not in 'input' group");
            warn!("Run: sudo usermod -a -G input $USER");
            warn!("Then logout and login again");
        }
    }

    Ok(())
}
