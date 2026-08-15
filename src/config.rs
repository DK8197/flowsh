//! Config file support. Reads `~/.config/shdev/config.toml` (or the
//! platform-appropriate equivalent via the `dirs` crate) if present.
//!
//! Design principle: **a missing or malformed config file is never a
//! hard error.** shdev must always start with sensible defaults —
//! config is an opt-in convenience, not a requirement. A malformed file
//! produces a one-line warning (surfaced via the initial status
//! message) and falls back to defaults for every field, rather than
//! refusing to start or silently guessing per-field.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The shell binary to spawn as the persistent session. Must behave
    /// enough like bash to accept `--noprofile`/`--norc`/`--noediting`
    /// and the `stty`/`HISTCONTROL`/`printf` constructs the execution
    /// protocol relies on — see `.claude/steering/gotchas.md` before
    /// changing the default or testing a different shell here. `sh`,
    /// `dash`, and similar minimal shells are **not** drop-in
    /// replacements; they don't all support the same flags or `printf`
    /// behavior this protocol depends on.
    pub shell: String,
    /// Extra arguments passed to the shell at spawn, appended after the
    /// three shdev always passes (`--noprofile --norc --noediting`).
    /// Only meaningful if your chosen `shell` accepts them the same way
    /// bash does.
    pub shell_args: Vec<String>,
    /// Safety-net ceiling on any single command's runtime, in seconds.
    /// Manual Ctrl+C is the primary cancellation mechanism; this only
    /// guards against a forgotten long-running command. See
    /// `executor::MAX_RUNTIME`.
    pub command_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shell: "bash".to_string(),
            shell_args: Vec::new(),
            command_timeout_secs: crate::executor::DEFAULT_MAX_RUNTIME.as_secs(),
        }
    }
}

pub struct LoadedConfig {
    pub config: Config,
    /// Set if a config file existed but couldn't be parsed — shown once
    /// in the status bar at startup rather than failing to start.
    pub warning: Option<String>,
}

impl Config {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("shdev").join("config.toml"))
    }

    /// Load the config file if present, falling back to defaults for
    /// anything missing, invalid, or entirely absent. Never returns an
    /// `Err` — see the module doc comment for why.
    pub fn load() -> LoadedConfig {
        let Some(path) = Self::config_path() else {
            return LoadedConfig { config: Config::default(), warning: None };
        };
        if !path.exists() {
            return LoadedConfig { config: Config::default(), warning: None };
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(config) => LoadedConfig { config, warning: None },
                Err(e) => LoadedConfig {
                    config: Config::default(),
                    warning: Some(format!("Config at {} is invalid, using defaults: {e}", path.display())),
                },
            },
            Err(e) => LoadedConfig {
                config: Config::default(),
                warning: Some(format!("Couldn't read config at {}, using defaults: {e}", path.display())),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let c = Config::default();
        assert_eq!(c.shell, "bash");
        assert!(c.shell_args.is_empty());
        assert_eq!(c.command_timeout_secs, 900);
    }

    #[test]
    fn partial_toml_fills_in_defaults() {
        let c: Config = toml::from_str("shell = \"zsh\"").unwrap();
        assert_eq!(c.shell, "zsh");
        assert_eq!(c.command_timeout_secs, 900); // untouched field keeps its default
    }

    #[test]
    fn invalid_toml_is_reported_not_fatal() {
        let result = toml::from_str::<Config>("shell = [this is not valid toml");
        assert!(result.is_err());
    }
}
