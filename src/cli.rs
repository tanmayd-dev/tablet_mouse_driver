//! Command-line interface.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};
use clap_complete::Shell;

use crate::config::{Mode, MouseScheme, PedalScheme, SteeringAxis};

/// Tablet input to virtual mouse / Xbox 360 gamepad bridge.
#[derive(Debug, Parser)]
#[command(
    name = "tablet-driver",
    version,
    about = "Translate Wacom tablet input into mouse or Xbox 360 gamepad input",
    long_about = None,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,

    /// Run options. `tablet-driver --preset minecraft` is shorthand for
    /// `tablet-driver run --preset minecraft`.
    #[command(flatten)]
    pub run: RunArgs,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Run the input bridge.
    Run(RunArgs),
    /// List detected tablet input devices.
    Probe,
    /// List available presets.
    Presets(PresetsArgs),
    /// Print or install shell completion.
    Completion(CompletionArgs),
    /// Print the fully resolved configuration as TOML.
    Config(RunArgs),
}

/// Options accepted by `run` (and by the default invocation).
#[derive(Debug, Clone, Default, Args)]
pub struct RunArgs {
    /// Preset name (built-in or from the user preset directory).
    #[arg(short, long, value_name = "PRESET")]
    pub preset: Option<String>,

    /// Config file overlaid on top of the preset.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Pen input device path (default: auto-detect).
    #[arg(short, long, value_name = "PATH")]
    pub device: Option<PathBuf>,

    /// ExpressKey pad input device path (default: auto-detect).
    #[arg(long, value_name = "PATH")]
    pub pad_device: Option<PathBuf>,

    /// Translation mode.
    #[arg(long, value_enum, value_name = "MODE")]
    pub mode: Option<Mode>,

    /// Mouse motion scheme (mouse mode only).
    #[arg(long, value_enum, value_name = "SCHEME")]
    pub scheme: Option<MouseScheme>,

    /// Mouse sensitivity multiplier.
    #[arg(long, value_name = "FACTOR")]
    pub sensitivity: Option<f64>,

    /// Raw tablet counts to mouse counts.
    #[arg(long, value_name = "FACTOR")]
    pub scale: Option<f64>,

    /// Delta mode: reject single-sample coordinate jumps above this (0 disables).
    #[arg(long, value_name = "COUNTS")]
    pub max_jump: Option<i32>,

    /// Invert horizontal mouse motion.
    #[arg(long)]
    pub invert_x: bool,

    /// Invert vertical mouse motion.
    #[arg(long)]
    pub invert_y: bool,

    /// Grab the pen device exclusively.
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "no_grab")]
    pub grab: bool,

    /// Do not grab the pen device.
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "grab")]
    pub no_grab: bool,

    /// Grab the ExpressKey pad exclusively.
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "no_grab_pad")]
    pub grab_pad: bool,

    /// Do not grab the ExpressKey pad.
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "grab_pad")]
    pub no_grab_pad: bool,

    /// Enable tablet throttle and brake.
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "no_pedals")]
    pub pedals: bool,

    /// Disable tablet throttle and brake (keyboard pedals work untouched).
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "pedals")]
    pub no_pedals: bool,

    /// Tablet pedal scheme.
    #[arg(long, value_enum, value_name = "SCHEME")]
    pub pedal_scheme: Option<PedalScheme>,

    /// Tablet axis used for steering.
    #[arg(long, value_enum, value_name = "AXIS")]
    pub steering_axis: Option<SteeringAxis>,

    /// Steering response exponent (1.0 = linear).
    #[arg(long, value_name = "FACTOR")]
    pub steering_curve: Option<f64>,

    /// Steering deadzone in [0.0, 1.0).
    #[arg(long, value_name = "VALUE")]
    pub steering_deadzone: Option<f64>,

    /// Steering centre in [0.0, 1.0].
    #[arg(long, value_name = "VALUE")]
    pub steering_center: Option<f64>,

    /// Distance from centre that reaches full lock.
    #[arg(long, value_name = "VALUE")]
    pub steering_half_span: Option<f64>,

    /// Invert steering direction.
    #[arg(long)]
    pub invert_steering: bool,

    /// Log events without creating virtual devices.
    #[arg(long)]
    pub dry_run: bool,

    /// Increase log verbosity (repeatable).
    #[arg(short, long, action = ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Debug, Clone, Default, Args)]
pub struct PresetsArgs {
    /// Print preset names only (used by shell completion).
    #[arg(long)]
    pub plain: bool,
}

#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// Shell to generate completion for.
    #[arg(value_enum)]
    pub shell: Shell,

    /// Install the completion file into the user's completion directory.
    #[arg(long)]
    pub install: bool,
}

const COMMANDS: &[&str] = &["run", "probe", "presets", "completion", "config"];

/// Parses the command line, inserting the default `run` subcommand when the
/// first argument is an option or absent.
pub fn parse() -> Cli {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    if needs_default_subcommand(&args) {
        args.insert(1, OsString::from("run"));
    }
    Cli::parse_from(args)
}

fn needs_default_subcommand(args: &[OsString]) -> bool {
    let Some(first) = args.get(1).and_then(|arg| arg.to_str()) else {
        return true;
    };
    if matches!(first, "-h" | "--help" | "-V" | "--version") {
        return false;
    }
    !COMMANDS.contains(&first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_run_for_option_first_invocations() {
        let args: Vec<OsString> = ["tablet-driver", "--preset", "minecraft"]
            .iter()
            .map(OsString::from)
            .collect();
        assert!(needs_default_subcommand(&args));

        let args: Vec<OsString> = ["tablet-driver"].iter().map(OsString::from).collect();
        assert!(needs_default_subcommand(&args));
    }

    #[test]
    fn keeps_explicit_subcommands() {
        for command in COMMANDS {
            let args: Vec<OsString> = ["tablet-driver", command]
                .iter()
                .map(OsString::from)
                .collect();
            assert!(!needs_default_subcommand(&args), "{command}");
        }
        let args: Vec<OsString> = ["tablet-driver", "--help"]
            .iter()
            .map(OsString::from)
            .collect();
        assert!(!needs_default_subcommand(&args));
    }

    #[test]
    fn parses_run_options() {
        let cli = Cli::try_parse_from([
            "tablet-driver",
            "run",
            "--preset",
            "trackmania",
            "--no-pedals",
            "--steering-curve",
            "1.5",
            "--max-jump",
            "3000",
            "--dry-run",
        ])
        .unwrap();
        match cli.command {
            Some(CliCommand::Run(args)) => {
                assert_eq!(args.preset.as_deref(), Some("trackmania"));
                assert!(args.no_pedals);
                assert_eq!(args.steering_curve, Some(1.5));
                assert_eq!(args.max_jump, Some(3000));
                assert!(args.dry_run);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_completion_shell() {
        let cli = Cli::try_parse_from(["tablet-driver", "completion", "zsh", "--install"]).unwrap();
        match cli.command {
            Some(CliCommand::Completion(args)) => {
                assert_eq!(args.shell, Shell::Zsh);
                assert!(args.install);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
