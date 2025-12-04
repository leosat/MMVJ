#![cfg_attr(all(not(feature = "default"), not(debug_assertions)), allow(warnings))]

use anyhow::{Result, bail};
use clap::{ArgAction, Parser};
use mmvj_lib::config;

fn print_app_version_and_built_with_features_banner() {
    const FEATURES: &[(&str, bool)] = &[
        (
            "HID (Mice/Keyboards/Joysticks) input and output, Virtual HID creation",
            true,
        ),
        (
            "MIDI input (optional, build feature flag: ``midi'')",
            cfg!(feature = "midi"),
        ),
        ("Gui (optional, build feature flag: ``gui'')", cfg!(feature = "gui")),
    ];

    let line = "________________________";
    println!();
    println!(
        " {} v. {}\n  built with features:",
        config::APP_NAME,
        env!("CARGO_PKG_VERSION")
    );
    for (name, enabled) in FEATURES {
        println!("    - {}: {}", name, if *enabled { "Yes" } else { "No" });
    }
    println!("{}\n", line);
}

#[derive(Parser)]
#[command(name = mmvj_lib::config::APP_NAME)]
#[command(bin_name = mmvj_lib::config::APP_COMMAND_NAME)]
#[command(author = mmvj_lib::config::APP_AUTHORS)]
#[command(version = mmvj_lib::config::APP_VERSION_STR)]
#[command(about = mmvj_lib::config::APP_ABOUT, long_about = mmvj_lib::config::APP_LONG_ABOUT)]
struct Cli {
    #[arg(long, help = "Log to console even if running Gui.", num_args = 0..=1, 
    default_value = "false")]
    log_to_console: bool,
    #[arg(short, long = "config", alias = "cfg-file-path", default_value = mmvj_lib::config::APP_DEFAULT_CONFIG_FILE,
    help = "Path to main config file, including filename.",
    overrides_with = "cfg_file_path", action = ArgAction::Set)]
    cfg_file_path: std::path::PathBuf,
    #[arg(long, default_value =  mmvj_lib::config::APP_DEFAULT_NO_HOT_RELOAD, help = "Disable automatic engine reload on configuration file change.")]
    no_hot_reload: bool,
    #[arg(
        value_enum,
        default_value_t = mmvj_lib::debug::DebugLevel::Off,
        short,
        long,
        help = "Enable debug information output (including related to Force Feedback, override with --debug-ff false)."
    )]
    debug: mmvj_lib::debug::DebugLevel,
    #[arg(long, help = "Enable Force Feedback debug information output.", num_args = 0..=1, 
    default_value = "false")]
    debug_ff: bool,
    #[arg(
        long,
        help = "Enable debug output for routines being run at idle tick when no user input \
        (to debug e.g. autocentering during steering transformation)."
    )]
    debug_idle_tick: bool,
    #[arg(
        short = 'u',
        long,
        help = "Rate of processing when no user input (idle tick), in Hz."
    )]
    idle_tick_update_rate: Option<u32>,
    // #[arg(short = 'l', long, default_value = mmvj_lib::config::APP_DEFAULT_LATENCY_STR)]
    // latency_mode: String,
    #[arg(long, default_value = mmvj_lib::config::APP_DEFAULT_MAX_LOG_LEVEL, help = "Limit max log level.")]
    log_level: String,
    #[cfg(feature = "gui")]
    #[arg(long, alias = "gui-monitor", help = "Show monitors Gui (steering axis indicators, etc.).", 
        num_args = 0..=1, default_value = "false")]
    gui_monitors: bool,
    #[cfg(feature = "gui")]
    #[arg(long, alias = "gui", help = "Show full Gui: monitors + interactive configuration editor/debugger.",
    num_args = 0..=1, default_value = "false")]
    gui_full: bool,
    // TODO: enable_steering_indicator_console: bool,
    #[arg(
        long,
        help = "List of joystick keys to keep persistent, or 'all'.",
        num_args = 0..
    )]
    persistent_joysticks: Option<Vec<String>>,

    #[command(subcommand)]
    aux_task: Option<mmvj_lib::driver::AuxDriverTask>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // -------------------------------------------
    // console_subscriber::init();

    // -------------------------------------------
    print_app_version_and_built_with_features_banner();

    // -------------------------------------------
    let cli = Cli::parse();

    // -------------------------------------------
    {
        let mut logger_initialized = false;

        #[cfg(feature = "gui")]
        if cli.gui_full && !cli.log_to_console {
            egui_logger::builder().init()?;
            logger_initialized = true;
        }

        if !logger_initialized {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&cli.log_level))
                .filter_module("mmvj_lib", log::LevelFilter::Debug)
                .init();
        }
    }

    log::info!("------------====--=-=--=--==--====-=-=--==-=--===-=---=------------");
    log::info!(
        "Starting {}, version: {}.",
        mmvj_lib::config::APP_LONG_NAME,
        mmvj_lib::config::APP_VERSION_STR
    );
    log::info!("Re-run with -h if any help required.");
    if cli.debug.is_on() {
        log::debug!("General debug output enabled.");
    }
    if cli.debug_ff {
        log::debug!("Force feedback debug output enabled.");
    }
    log::info!("------------====--=-=--=--==--====-=-=--==-=--===-=---=------------");

    if cfg!(target_os = "linux") {
        mmvj_lib::driver::check_linux_system_requirements()?;
    } else {
        bail!("This application requires Linux.");
    }

    if let Some(ref aux_task) = cli.aux_task {
        return mmvj_lib::driver::run_aux_task(aux_task, &cli.cfg_file_path, cli.debug).await;
    }

    mmvj_lib::driver::run(
        &cli.cfg_file_path,
        cli.no_hot_reload,
        cli.debug,
        cli.debug_ff,
        cli.debug_idle_tick,
        cli.idle_tick_update_rate,
        cli.persistent_joysticks,
        #[cfg(feature = "gui")]
        cli.gui_monitors,
        #[cfg(feature = "gui")]
        cli.gui_full,
    )
    .await
}
