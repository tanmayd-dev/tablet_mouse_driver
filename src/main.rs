//! `tablet-driver` binary entry point.

use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tablet_mouse_driver::cli::{self, CliCommand, RunArgs};
use tablet_mouse_driver::config::Config;
use tablet_mouse_driver::{completion, device, engine, presets};

fn main() -> ExitCode {
    let cli = cli::parse();

    match cli.command {
        Some(CliCommand::Run(args)) => run(args),
        Some(CliCommand::Probe) => probe(),
        Some(CliCommand::Presets(args)) => list_presets(args.plain),
        Some(CliCommand::Completion(args)) => match completion::run(args.shell, args.install) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(error),
        },
        Some(CliCommand::Config(args)) => print_config(args),
        None => run(cli.run),
    }
}

fn init_logging(verbose: u8, dry_run: bool) {
    let default = match (verbose, dry_run) {
        (0, true) => "debug",
        (0, false) => "info",
        (1, _) => "debug",
        _ => "trace",
    };
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default))
        .try_init();
}

fn run(args: RunArgs) -> ExitCode {
    init_logging(args.verbose, args.dry_run);

    let config = match Config::resolve(&args) {
        Ok(config) => config,
        Err(error) => return fail(error),
    };

    log::debug!(
        "resolved config: preset={:?} mode={} grab_pen={} grab_pad={}",
        config.preset,
        config.general.mode,
        config.general.grab_pen,
        config.general.grab_pad
    );

    let term = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        if let Err(error) = signal_hook::flag::register(signal, Arc::clone(&term)) {
            eprintln!("warning: cannot install signal handler: {error}");
        }
    }

    match engine::run(config, args.dry_run, term) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn probe() -> ExitCode {
    let devices = device::scan_devices();
    if devices.is_empty() {
        println!("No input devices found under /dev/input.");
        return ExitCode::SUCCESS;
    }
    println!("Detected input devices (highest score first):");
    for device in devices {
        println!("  {device}");
    }
    ExitCode::SUCCESS
}

fn list_presets(plain: bool) -> ExitCode {
    let entries = presets::list();
    if plain {
        for entry in entries {
            println!("{}", entry.name);
        }
        return ExitCode::SUCCESS;
    }

    println!("{:<14} SOURCE", "PRESET");
    for entry in entries {
        println!("{:<14} {}", entry.name, entry.source);
    }
    if let Some(dir) = presets::user_dir() {
        println!("\nUser presets: {}", dir.display());
    }
    ExitCode::SUCCESS
}

fn print_config(args: RunArgs) -> ExitCode {
    init_logging(args.verbose, args.dry_run);
    let config = match Config::resolve(&args) {
        Ok(config) => config,
        Err(error) => return fail(error),
    };
    match toml::to_string_pretty(&config) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => fail(error),
    }
}

fn fail(error: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {error}");
    ExitCode::FAILURE
}
