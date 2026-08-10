#![cfg_attr(all(not(feature = "default"), not(debug_assertions)), allow(warnings))]

use anyhow::{Result, bail};
use clap::Parser;
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

#[tokio::main]
async fn main() -> Result<()> {
    // -------------------------------------------
    // console_subscriber::init();

    // -------------------------------------------
    print_app_version_and_built_with_features_banner();

    // -------------------------------------------
    let cli = mmvj_lib::cli::Cli::parse();

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
