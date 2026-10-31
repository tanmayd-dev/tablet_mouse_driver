//! Shell completion generation and installation.
//!
//! Zsh gets a hand-written completion function because it can call
//! `tablet-driver presets --plain` for dynamic preset names. Bash and fish use
//! clap's generator.

use std::fs;
use std::io;
use std::path::PathBuf;

use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::Cli;

const BIN: &str = "tablet-driver";

/// Hand-written zsh completion with dynamic preset names.
pub const ZSH_SCRIPT: &str = r#"#compdef tablet-driver

_tablet_driver_presets() {
  local -a presets
  presets=(${(f)"$(tablet-driver presets --plain 2>/dev/null)"})
  (( ${#presets} )) || presets=(minecraft trackmania small-bad desktop)
  _describe -t presets 'preset' presets
}

_tablet_driver_commands() {
  local -a commands
  commands=(
    'run:Run the tablet input bridge'
    'probe:List detected tablet input devices'
    'presets:List available presets'
    'completion:Print or install shell completion'
    'config:Print the fully resolved configuration'
  )
  _describe -t commands 'tablet-driver command' commands
}

_tablet_driver_run_options() {
  _arguments -s \
    '(-h --help)'{-h,--help}'[Print help]' \
    '(-V --version)'{-V,--version}'[Print version]' \
    '(-p --preset)'{-p,--preset}'[Preset to load]:preset:_tablet_driver_presets' \
    '(-c --config)'{-c,--config}'[Config file overlaid on the preset]:file:_files' \
    '(-d --device)'{-d,--device}'[Pen input device path]:device:_files -g "*/event*"' \
    '--pad-device[ExpressKey pad input device path]:device:_files -g "*/event*"' \
    '--mode[Translation mode]:mode:(mouse gamepad)' \
    '--scheme[Mouse motion scheme]:scheme:(delta rate)' \
    '--sensitivity[Mouse sensitivity multiplier]:factor:' \
    '--scale[Raw tablet counts to mouse counts]:factor:' \
    '--invert-x[Invert horizontal mouse motion]' \
    '--invert-y[Invert vertical mouse motion]' \
    '--grab[Grab the pen device exclusively]' \
    '--no-grab[Do not grab the pen device]' \
    '--grab-pad[Grab the ExpressKey pad exclusively]' \
    '--no-grab-pad[Do not grab the ExpressKey pad]' \
    '--pedals[Enable tablet throttle and brake]' \
    '--no-pedals[Disable tablet throttle and brake]' \
    '--pedal-scheme[Tablet pedal scheme]:scheme:(split pressure combined)' \
    '--steering-axis[Tablet axis used for steering]:axis:(x y)' \
    '--steering-curve[Steering response exponent]:factor:' \
    '--steering-deadzone[Steering deadzone in 0..1]:value:' \
    '--steering-center[Steering centre in 0..1]:value:' \
    '--steering-half-span[Distance from centre to full lock]:value:' \
    '--invert-steering[Invert steering direction]' \
    '--dry-run[Log events without creating virtual devices]' \
    '(-v --verbose)'{-v,--verbose}'[Increase log verbosity]'
}

_tablet_driver() {
  local -a commands
  commands=(run probe presets completion config)

  if (( CURRENT == 2 )); then
    _alternative \
      'commands:command:_tablet_driver_commands' \
      'options:option:_tablet_driver_run_options'
    return
  fi

  case $words[2] in
    run)
      words=( $words[1] $words[3,-1] )
      (( CURRENT-- ))
      _tablet_driver_run_options
      ;;
    probe)
      _arguments '(-h --help)'{-h,--help}'[Print help]'
      ;;
    presets)
      _arguments '--plain[Print preset names only]' '(-h --help)'{-h,--help}'[Print help]'
      ;;
    completion)
      _arguments \
        '--install[Install the completion file]' \
        '(-h --help)'{-h,--help}'[Print help]' \
        '1:shell:(zsh bash fish)'
      ;;
    config)
      _arguments \
        '(-p --preset)'{-p,--preset}'[Preset to load]:preset:_tablet_driver_presets' \
        '(-c --config)'{-c,--config}'[Config file]:file:_files' \
        '(-h --help)'{-h,--help}'[Print help]'
      ;;
    *)
      _tablet_driver_run_options
      ;;
  esac
}

_tablet_driver "$@"
"#;

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn install_path(shell: Shell) -> Option<PathBuf> {
    match shell {
        Shell::Zsh => Some(data_home()?.join("zsh/site-functions/_tablet-driver")),
        Shell::Bash => Some(data_home()?.join("bash-completion/completions/tablet-driver")),
        Shell::Fish => Some(config_home()?.join("fish/completions/tablet-driver.fish")),
        _ => None,
    }
}

fn generated(shell: Shell) -> String {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    clap_complete::generate(shell, &mut command, BIN, &mut buffer);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// Prints or installs completion for `shell`.
pub fn run(shell: Shell, install: bool) -> io::Result<()> {
    let content = match shell {
        Shell::Zsh => ZSH_SCRIPT.to_string(),
        other => generated(other),
    };

    if !install {
        print!("{content}");
        return Ok(());
    }

    let path = install_path(shell).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("completion installation is not supported for {shell}"),
        )
    })?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    println!("Installed {shell} completion to {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_script_is_syntactically_plausible() {
        assert!(ZSH_SCRIPT.starts_with("#compdef tablet-driver"));
        assert!(ZSH_SCRIPT.contains("_tablet_driver_presets"));
        assert!(ZSH_SCRIPT.contains("tablet-driver presets --plain"));
        assert!(ZSH_SCRIPT.contains("_tablet_driver \"$@\""));
        assert!(ZSH_SCRIPT.contains("--no-pedals"));
    }

    #[test]
    fn generated_bash_completion_mentions_the_binary() {
        let script = generated(Shell::Bash);
        assert!(script.contains("tablet-driver"));
    }
}
