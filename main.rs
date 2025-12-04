use anyhow::Result;
use clap::Parser;

mod common;
mod config;
mod driver;
mod interpolation;
mod joystick;
mod mapping;
mod midi;
mod mouse;
mod schemas;

#[derive(Parser)]
#[command(name = config::APP_NAME)]
#[command(author = config::APP_AUTHORS)]
#[command(version = config::APP_VERSION_STR)]
#[command(about = config::APP_ABOUT, long_about = config::APP_LONG_ABOUT)]
struct Cli {
    #[arg(short, long, default_value = config::APP_DEFAULT_CONFIG_FILE)]
    config_path: std::path::PathBuf,
    #[arg(short, long)]
    debug: bool,
    #[arg(short = 'u', long, default_value = config::APP_DEFAULT_UPDATE_FREQ_HZ_STR)]
    update_rate_hz: u32,
    #[arg(short = 'l', long, default_value = config::APP_DEFAULT_LATENCY_STR)]
    latency_mode: String,
    #[arg(long, default_value = config::APP_DEFAULT_MAX_LOG_LEVEL)]
    log_level: String,
    #[arg(long)]
    debug_ff: bool,
    #[arg(long)]
    debug_idle_tick: bool,
    #[command(subcommand)]
    aux_task: Option<crate::driver::AuxDriverTask>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&cli.log_level))
        .init();

    log::info!("------------====--=-=--=--==--====-=-=--==-=--===-=---=------------");
    log::info!("Starting {}.", crate::config::APP_LONG_NAME);
    log::info!("Re-run with -h if any help required.");
    log::info!("------------====--=-=--=--==--====-=-=--==-=--===-=---=------------");

    crate::driver::check_system_requirements()?;

    if let Some(ref aux_task) = cli.aux_task {
        return crate::driver::run_aux_task(aux_task, cli.config_path, cli.debug).await;
    }

    crate::driver::run_mapping_engine(
        cli.config_path,
        cli.debug,
        cli.debug_ff,
        cli.debug_idle_tick,
        cli.update_rate_hz,
    )
    .await
}
