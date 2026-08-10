use clap::{ArgAction, Parser};

#[derive(Parser)]
#[command(name = crate::config::APP_NAME)]
#[command(bin_name = crate::config::APP_COMMAND_NAME)]
#[command(author = crate::config::APP_AUTHORS)]
#[command(version = crate::config::APP_VERSION_STR)]
#[command(about = crate::config::APP_ABOUT, long_about = crate::config::APP_LONG_ABOUT)]
pub struct Cli {
    #[arg(long, help = "Log to console even if running Gui.", num_args = 0..=1, 
    default_value = "false")]
    pub log_to_console: bool,
    #[arg(short, long = "config", alias = "cfg-file-path", default_value = crate::config::APP_DEFAULT_CONFIG_FILE,
    help = "Path to main config file, including filename.",
    overrides_with = "cfg_file_path", action = ArgAction::Set)]
    pub cfg_file_path: std::path::PathBuf,
    #[arg(long, default_value =  crate::config::APP_DEFAULT_NO_HOT_RELOAD, help = "Disable automatic engine reload on configuration file change.")]
    pub no_hot_reload: bool,
    #[arg(
        value_enum,
        default_value_t = crate::debug::DebugLevel::Off,
        short,
        long,
        help = "Enable debug information output (including related to Force Feedback, override with --debug-ff false)."
    )]
    pub debug: crate::debug::DebugLevel,
    #[arg(long, help = "Enable Force Feedback debug information output.", num_args = 0..=1, 
    default_value = "false")]
    pub debug_ff: bool,
    #[arg(
        long,
        help = "Enable debug output for routines being run at idle tick when no user input \
        (to debug e.g. autocentering during steering transformation)."
    )]
    pub debug_idle_tick: bool,
    #[arg(
        short = 'u',
        long,
        help = "Rate of processing when no user input (idle tick), in Hz."
    )]
    pub idle_tick_update_rate: Option<u32>,
    // #[arg(short = 'l', long, default_value = crate::config::APP_DEFAULT_LATENCY_STR)]
    // latency_mode: String,
    #[arg(long, default_value = crate::config::APP_DEFAULT_MAX_LOG_LEVEL, help = "Limit max log level.")]
    pub log_level: String,
    #[cfg(feature = "gui")]
    #[arg(long, alias = "gui-monitor", help = "Show monitors Gui (steering axis indicators, etc.).", 
        num_args = 0..=1, default_value = "false")]
    pub gui_monitors: bool,
    #[cfg(feature = "gui")]
    #[arg(long, alias = "gui", help = "Show full Gui: monitors + interactive configuration editor/debugger.",
    num_args = 0..=1, default_value = "false")]
    pub gui_full: bool,
    // TODO: enable_steering_indicator_console: bool,
    #[arg(
        long,
        help = "List of joystick keys to keep persistent, or 'all'.",
        num_args = 0..
    )]
    pub persistent_joysticks: Option<Vec<String>>,

    #[command(subcommand)]
    pub aux_task: Option<crate::driver::AuxDriverTask>,
}
