//! Credentials and authentication modes.
//!
//! API keys come from the provider's environment variable, or from
//! `<config dir>/credentials.toml` (written with 0600 permissions by
//! `embed ai login`). They are never read from or written to project
//! files. Subscription mode uses only the provider's official CLI and its own
//! login; EmbedIDE never touches browser cookies or session tokens.
use crate::{Error, ProviderId, Result};
use serde::Serialize;
use std::{collections::BTreeMap, fmt, str::FromStr};

/// A credential whose Debug/Display never reveal it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a value.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// The raw value; only for building a request header.
    pub fn expose(&self) -> &str {
        &self.0
    }
    /// Replace every occurrence of the secret in `text`.
    pub fn redact(&self, text: &str) -> String {
        if self.0.len() < 4 {
            return text.to_string();
        }
        text.replace(&self.0, "[REDACTED]")
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}
impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// How to authenticate with a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// User-provided API key (billed per request by the provider).
    ApiKey,
    /// The provider's official CLI with the user's subscription login.
    Subscription,
}

impl FromStr for AuthMode {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "api-key" | "api" | "key" => Ok(Self::ApiKey),
            "subscription" | "cli" => Ok(Self::Subscription),
            _ => Err(format!(
                "Unknown auth mode '{s}'; expected api-key or subscription"
            )),
        }
    }
}

/// Where a key was found (for status display, never the key itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeySource {
    /// Provider environment variable.
    Environment,
    /// credentials.toml
    CredentialsFile,
    /// Not configured.
    Missing,
}

fn credentials_path() -> Result<std::path::PathBuf> {
    Ok(embedide_workspace::config_dir()
        .map_err(|e| Error::Config(e.0))?
        .join("credentials.toml"))
}

fn read_credentials() -> Result<BTreeMap<String, String>> {
    let path = credentials_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => {
            return Err(Error::Config(format!(
                "Cannot read {}: {e}",
                path.display()
            )))
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(&path) {
            if m.permissions().mode() & 0o077 != 0 {
                return Err(Error::Config(format!(
                    "{} is readable by other users; run `chmod 600 {}` before EmbedIDE will use it",
                    path.display(),
                    path.display()
                )));
            }
        }
    }
    let table: toml::Table =
        toml::from_str(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
    Ok(table
        .iter()
        .filter_map(|(k, v)| {
            v.get("api_key")
                .and_then(|x| x.as_str())
                .map(|s| (k.clone(), s.to_string()))
        })
        .collect())
}

/// The API key for a provider, if configured.
pub fn api_key(p: ProviderId) -> Result<Option<Secret>> {
    if let Some(k) = std::env::var(p.info().key_env)
        .ok()
        .filter(|k| !k.trim().is_empty())
    {
        return Ok(Some(Secret::new(k.trim())));
    }
    Ok(read_credentials()?.remove(p.id()).map(Secret::new))
}

/// Where the key would come from.
pub fn key_source(p: ProviderId) -> KeySource {
    if std::env::var(p.info().key_env).is_ok_and(|k| !k.trim().is_empty()) {
        KeySource::Environment
    } else if read_credentials().is_ok_and(|c| c.contains_key(p.id())) {
        KeySource::CredentialsFile
    } else {
        KeySource::Missing
    }
}

/// Store a key in the private credentials file.
pub fn store_key(p: ProviderId, key: &Secret) -> Result<std::path::PathBuf> {
    let k = key.expose().trim();
    if k.len() < 8 || k.contains(char::is_whitespace) {
        return Err(Error::Config(
            "That does not look like an API key (too short or contains whitespace)".into(),
        ));
    }
    let mut all = read_credentials().unwrap_or_default();
    all.insert(p.id().to_string(), k.to_string());
    write_credentials(&all)
}

/// Remove a stored key; returns whether one existed.
pub fn remove_key(p: ProviderId) -> Result<bool> {
    let mut all = read_credentials()?;
    let existed = all.remove(p.id()).is_some();
    if existed {
        write_credentials(&all)?;
    }
    Ok(existed)
}

fn write_credentials(all: &BTreeMap<String, String>) -> Result<std::path::PathBuf> {
    let mut table = toml::Table::new();
    for (p, k) in all {
        let mut t = toml::Table::new();
        t.insert("api_key".into(), toml::Value::String(k.clone()));
        table.insert(p.clone(), toml::Value::Table(t));
    }
    let text = format!(
        "# EmbedIDE API keys. Keep this file private (0600). Never commit it.\n{}",
        toml::to_string(&table).map_err(|e| Error::Config(e.to_string()))?
    );
    let path = credentials_path()?;
    embedide_workspace::write_atomic(&path, text.as_bytes(), true)
        .map_err(|e| Error::Config(e.0))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_print() {
        let s = Secret::new("sk-ant-very-secret-value");
        assert_eq!(format!("{s}"), "[REDACTED]");
        assert_eq!(format!("{s:?}"), "Secret([REDACTED])");
        assert_eq!(
            s.redact("bad key sk-ant-very-secret-value here"),
            "bad key [REDACTED] here"
        );
    }
}
