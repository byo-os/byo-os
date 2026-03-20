//! Configuration loading for the orchestrator.
//!
//! Reads a TOML config file describing which processes to spawn.

use serde::Deserialize;
use std::path::Path;

/// Top-level config.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub process: Vec<ProcessConfig>,
}

/// A process to spawn and manage.
#[derive(Debug, Deserialize)]
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Config {
    /// Load config from a TOML file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::parse(&content)
    }

    /// Parse config from a TOML string.
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        toml::from_str(content).map_err(ConfigError::Toml)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Toml(toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config IO error: {e}"),
            ConfigError::Toml(e) => write!(f, "config parse error: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_config() {
        let toml = r#"
            [[process]]
            name = "compositor"
            command = "byo-compositor"

            [[process]]
            name = "controls"
            command = "byo-controls"
        "#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.process.len(), 2);
        assert_eq!(config.process[0].name, "compositor");
        assert_eq!(config.process[0].command, "byo-compositor");
        assert!(config.process[0].args.is_empty());
        assert_eq!(config.process[1].name, "controls");
    }

    #[test]
    fn parse_with_args() {
        let toml = r#"
            [[process]]
            name = "compositor"
            command = "byo-compositor"
            args = ["--fullscreen", "--scale=2"]
        "#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.process[0].args, vec!["--fullscreen", "--scale=2"]);
    }

    #[test]
    fn parse_empty_process_list() {
        let toml = "process = []\n";
        let config = Config::parse(toml).unwrap();
        assert!(config.process.is_empty());
    }

    #[test]
    fn parse_invalid_toml() {
        assert!(Config::parse("not valid toml [[[").is_err());
    }

    #[test]
    fn parse_missing_required_field() {
        let toml = r#"
            [[process]]
            name = "compositor"
        "#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn parse_system_toml() {
        let content = include_str!("../config/system.toml");
        let config = Config::parse(content).unwrap();
        assert!(config.process.len() >= 2);
        assert_eq!(config.process[0].name, "compositor");
        let names: Vec<&str> = config.process.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"controls"), "config should contain controls process");
    }
}
