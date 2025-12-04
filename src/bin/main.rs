use anyhow::{bail, Result};
use clap::Parser;

#[derive(Parser)]
#[command(name = mmvj_lib::config::APP_NAME)]
#[command(author = mmvj_lib::config::APP_AUTHORS)]
#[command(version = mmvj_lib::config::APP_VERSION_STR)]
#[command(about = mmvj_lib::config::APP_ABOUT, long_about = mmvj_lib::config::APP_LONG_ABOUT)]
struct Cli {
    #[arg(short, long, default_value = mmvj_lib::config::APP_DEFAULT_CONFIG_FILE)]
    config_path: std::path::PathBuf,
    #[arg(short, long)]
    debug: bool,
    #[arg(short = 'u', long, default_value = mmvj_lib::config::APP_DEFAULT_UPDATE_FREQ_HZ_STR)]
    update_rate_hz: u32,
    #[arg(short = 'l', long, default_value = mmvj_lib::config::APP_DEFAULT_LATENCY_STR)]
    latency_mode: String,
    #[arg(long, default_value = mmvj_lib::config::APP_DEFAULT_MAX_LOG_LEVEL)]
    log_level: String,
    #[arg(long)]
    debug_ff: bool,
    #[arg(long)]
    debug_idle_tick: bool,
    #[command(subcommand)]
    aux_task: Option<mmvj_lib::driver::AuxDriverTask>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = {
        let mut cli = Cli::parse();
        cli.debug_ff |= cli.debug;
        cli
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&cli.log_level))
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
        return mmvj_lib::driver::run_aux_task(aux_task, cli.config_path, cli.debug).await;
    }

    mmvj_lib::driver::run_mapping_engine(
        cli.config_path,
        cli.debug,
        cli.debug_ff,
        cli.debug_idle_tick,
        cli.update_rate_hz,
    )
    .await
}
