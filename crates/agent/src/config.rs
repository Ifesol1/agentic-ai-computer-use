//! `harness.toml`: which models the harness can use. One `[providers.<name>]`
//! table per model (an [`llm::ProviderConfig`]), and `default = "<name>"`.
//!
//! Keys never live here: each table names the environment variable that holds
//! its key (`api_key_env`), and the adapter reads it when a run starts. The
//! built-in `mock` provider (the scripted model) is always available too, so
//! the app's picker and the tests have something that needs no key.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use anyhow::{Context, bail};
use llm::ProviderConfig;
use proto::ProviderInfo;
use serde::Deserialize;

use crate::ProviderSpec;

/// Where `harness` looks when `--config` isn't given.
pub const DEFAULT_PATH: &str = "harness.toml";

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Config {
    /// The provider a run uses when it names none. May be left out when there
    /// is exactly one.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("can't read {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(text)?;
        config.check()?;
        Ok(config)
    }

    /// Every table names a known adapter, and there is one default.
    fn check(&self) -> anyhow::Result<()> {
        for (name, provider) in &self.providers {
            if name == MOCK {
                bail!("`{MOCK}` is the built-in scripted model; pick another name");
            }
            let kinds: Vec<_> = llm::kinds().collect();
            if !kinds.contains(&provider.kind.as_str()) {
                bail!(
                    "[providers.{name}] kind = {:?} isn't an adapter; known kinds: {}",
                    provider.kind,
                    kinds.join(", ")
                );
            }
        }
        match (&self.default, self.providers.len()) {
            (Some(default), _) if !self.providers.contains_key(default) => {
                bail!("default = {default:?} names no [providers.{default}] table")
            }
            (None, n) if n > 1 => bail!("several providers: say which is the `default`"),
            _ => Ok(()),
        }
    }

    /// The default provider's name: `default`, else the only one, else `mock`.
    pub fn default_name(&self) -> &str {
        match (&self.default, self.providers.keys().next()) {
            (Some(default), _) => default,
            (None, Some(only)) if self.providers.len() == 1 => only,
            _ => MOCK,
        }
    }

    /// The configured providers plus the built-in mock, as the backend takes them.
    /// Each one is built once here, so a bad table fails now, not on a run.
    pub fn providers(&self) -> anyhow::Result<Vec<ProviderSpec>> {
        let default = self.default_name();
        let mut specs = vec![mock_spec(default == MOCK)];
        for (name, table) in &self.providers {
            let config = ProviderConfig {
                name: name.clone(),
                ..table.clone()
            };
            let provider = llm::build(&config).with_context(|| format!("[providers.{name}]"))?;
            let caps = provider.caps(&config.model);
            specs.push(ProviderSpec {
                info: ProviderInfo {
                    name: name.clone(),
                    kind: config.kind.clone(),
                    model: config.model.clone(),
                    vision: caps.vision,
                    native_computer: caps.native_computer,
                    is_default: name == default,
                },
                build: Arc::new(move || llm::build(&config)),
            });
        }
        Ok(specs)
    }
}

/// The built-in scripted model's name.
pub const MOCK: &str = "mock";

/// The scripted model that plays the demo, always available.
pub fn mock_spec(is_default: bool) -> ProviderSpec {
    ProviderSpec {
        info: ProviderInfo {
            name: MOCK.into(),
            kind: MOCK.into(),
            model: "scripted".into(),
            vision: true,
            native_computer: false,
            is_default,
        },
        build: Arc::new(|| Ok(Box::new(llm::MockProvider::new(crate::demo::script())))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../harness.example.toml");

    #[test]
    fn the_example_file_parses_and_lists_its_providers() {
        let config = Config::parse(EXAMPLE).unwrap();
        assert_eq!(config.default_name(), "claude");
        let claude = &config.providers["claude"];
        assert_eq!(claude.kind, "anthropic");
        assert_eq!(claude.api_key_env.as_deref(), Some("ANTHROPIC_API_KEY"));
        assert!(claude.allow_computer);

        let specs = config.providers().unwrap();
        let names: Vec<_> = specs.iter().map(|s| s.info.name.as_str()).collect();
        assert_eq!(names, ["mock", "claude"]);
        let claude = &specs[1].info;
        assert!(claude.is_default && !specs[0].info.is_default);
        assert!(claude.vision, "the adapter's default caps");
        assert_eq!(specs.iter().filter(|s| s.info.is_default).count(), 1);
        // Keys never reach the API's view of a provider.
        assert!(!format!("{claude:?}").contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn one_provider_needs_no_default() {
        let config = Config::parse(
            r#"
            [providers.claude]
            kind = "anthropic"
            model = "claude-opus-5-5"
            "#,
        )
        .unwrap();
        assert_eq!(config.default_name(), "claude");
    }

    #[test]
    fn no_providers_means_the_mock_is_the_default() {
        let config = Config::parse("").unwrap();
        assert_eq!(config.default_name(), MOCK);
        let specs = config.providers().unwrap();
        assert_eq!(specs.len(), 1);
        assert!(specs[0].info.is_default);
    }

    #[test]
    fn bad_tables_fail_at_load() {
        let two = r#"
            [providers.a]
            kind = "anthropic"
            [providers.b]
            kind = "anthropic"
        "#;
        assert!(
            Config::parse(two)
                .unwrap_err()
                .to_string()
                .contains("default")
        );

        let unknown = r#"
            [providers.x]
            kind = "nope"
        "#;
        let error = Config::parse(unknown).unwrap_err().to_string();
        assert!(
            error.contains("nope") && error.contains("anthropic"),
            "{error}"
        );

        let missing = r#"
            default = "y"
            [providers.x]
            kind = "anthropic"
        "#;
        assert!(Config::parse(missing).is_err());

        let taken = r#"
            [providers.mock]
            kind = "anthropic"
        "#;
        assert!(Config::parse(taken).is_err());

        let half = r#"
            [providers.x]
            kind = "anthropic"
            input_usd_per_mtok = 1.0
        "#;
        let error = match Config::parse(half).unwrap().providers() {
            Err(error) => format!("{error:#}"),
            Ok(_) => panic!("one price without the other built"),
        };
        assert!(error.contains("[providers.x]"), "{error}");
    }
}
