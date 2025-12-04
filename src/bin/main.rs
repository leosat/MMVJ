use anyhow::{bail, Result};
use clap::Parser;

#[derive(Parser)]
#[command(name = mmvj_lib::config::APP_NAME)]
#[command(bin_name = mmvj_lib::config::APP_COMMAND_NAME)]
#[command(author = mmvj_lib::config::APP_AUTHORS)]
#[command(version = mmvj_lib::config::APP_VERSION_STR)]
#[command(about = mmvj_lib::config::APP_ABOUT, long_about = mmvj_lib::config::APP_LONG_ABOUT)]
struct Cli {
    #[arg(short, long, default_value = mmvj_lib::config::APP_DEFAULT_CONFIG_FILE,
    help = "Path to main config file, including filename.")]
    cfg_file_path: std::path::PathBuf,
    #[arg(short, long, default_value = mmvj_lib::config::APP_DEFAULT_PREDEF_CONFIG_FILE_CFG_RELATIVE,
    help = "Path to predefines config file, including filename.")]
    predef_cfg_file_path: std::path::PathBuf,
    #[arg(long, default_value =  mmvj_lib::config::APP_DEFAULT_NO_HOT_RELOAD, help = "Disable automatic engine reload on configuration file change.")]
    no_hot_reload: bool,
    #[arg(
        short,
        long,
        help = "Enable debug information output (including related to Force Feedback, override with --debug-ff false)."
    )]
    debug: bool,
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
    #[arg(long, help = "Open a window with steering indicator", num_args = 0..=1, default_value = "false")]
    enable_steering_indicator_window: bool,
    // TODO: #[arg(long, help = "Show steering indicator in console", default_missing_value = "true")]
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
    let cli = Cli::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&cli.log_level))
        .filter_module("eframe", log::LevelFilter::Warn)
        .filter_module("winit", log::LevelFilter::Warn)
        .filter_module("tracing", log::LevelFilter::Warn)
        .filter_module("egui_winit", log::LevelFilter::Warn)
        .filter_module("egui_glow", log::LevelFilter::Warn)
        .filter_module("zbus", log::LevelFilter::Warn)
        .init();
    log::info!("------------====--=-=--=--==--====-=-=--==-=--===-=---=------------");
    log::info!("Starting {}.", mmvj_lib::config::APP_LONG_NAME);
    log::info!("Re-run with -h if any help required.");
    if cli.debug {
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
        return mmvj_lib::driver::run_aux_task(
            aux_task,
            &cli.cfg_file_path,
            &cli.predef_cfg_file_path,
            cli.debug,
        )
        .await;
    }

    mmvj_lib::driver::run_mapping_engine(
        &cli.cfg_file_path,
        &cli.predef_cfg_file_path,
        cli.no_hot_reload,
        cli.debug,
        cli.debug_ff,
        cli.debug_idle_tick,
        cli.idle_tick_update_rate,
        cli.persistent_joysticks,
        cli.enable_steering_indicator_window,
    )
    .await
}
