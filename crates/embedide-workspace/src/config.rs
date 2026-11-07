//! User configuration (`<config dir>/config.toml`) and dotted-key editing
//! shared with project manifests. Credentials are never stored here; see
//! `embedide-ai` for the separate credentials file.
use crate::{write_atomic, Error, Result};
use embedide_toolchains::CustomToolchain;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Per-user configuration directory. `EMBEDIDE_CONFIG_DIR` overrides it.
pub fn config_dir() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("EMBEDIDE_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let dir = match std::env::consts::OS {
        "windows" => std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("EmbedIDE")),
        "macos" => home().map(|h| h.join("Library/Application Support/EmbedIDE")),
        _ => std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home().map(|h| h.join(".config")))
            .map(|c| c.join("embedide")),
    };
    dir.ok_or_else(|| {
        Error("Cannot determine the configuration directory; set EMBEDIDE_CONFIG_DIR".into())
    })
}

/// `[toolchains]`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ToolchainsConfig {
    /// Extra directories searched after PATH.
    pub search_dirs: Vec<PathBuf>,
    /// User-defined toolchains (any path, any kind).
    pub custom: Vec<CustomToolchain>,
    /// Default toolchain id per language for projects that do not choose one.
    pub defaults: BTreeMap<String, String>,
}

/// `[ai.providers.<id>]`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProviderConfig {
    /// Override the API base URL (proxies, regional endpoints, tests).
    pub base_url: Option<String>,
    /// `api-key` or `subscription`.
    pub auth: Option<String>,
    /// Default model id for this provider.
    pub model: Option<String>,
    /// Path to the provider's official CLI for subscription mode.
    pub cli: Option<PathBuf>,
}

/// `[ai]`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AiConfig {
    /// Default provider id.
    pub provider: Option<String>,
    /// Model per task (`chat`, `complete`, `edit`), as `provider:model` or `model`.
    pub models: BTreeMap<String, String>,
    /// Provider settings.
    pub providers: BTreeMap<String, ProviderConfig>,
    /// Output token cap per request.
    pub max_tokens: u32,
    /// Upper bound on context bytes attached to one request.
    pub max_context_bytes: usize,
    /// Minimum delay between completion requests (debounce), milliseconds.
    pub completion_debounce_ms: u64,
    /// Suggest completions automatically while typing in the editor. Off by
    /// default: every suggestion is a provider request.
    pub inline: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: None,
            models: BTreeMap::new(),
            providers: BTreeMap::new(),
            max_tokens: 1024,
            max_context_bytes: 32 * 1024,
            completion_debounce_ms: 250,
            inline: false,
        }
    }
}

/// Everything in `config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UserConfig {
    /// `[toolchains]`
    pub toolchains: ToolchainsConfig,
    /// `[ai]`
    pub ai: AiConfig,
}

impl UserConfig {
    /// Path of `config.toml`.
    pub fn path() -> Result<PathBuf> {
        Ok(config_dir()?.join("config.toml"))
    }
    /// Load (defaults when the file does not exist).
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                Self::parse(&text).map_err(|e| Error(format!("{}: {}", path.display(), e.0)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error(format!("Cannot read {}: {e}", path.display()))),
        }
    }
    /// Parse and validate.
    pub fn parse(text: &str) -> Result<Self> {
        let c: Self =
            toml::from_str(text).map_err(|e| Error(format!("Invalid configuration: {e}")))?;
        for t in &c.toolchains.custom {
            if embedide_toolchains::kind(&t.kind).is_none() {
                return Err(Error(format!(
                    "toolchains.custom '{}': unknown kind '{}'",
                    t.id, t.kind
                )));
            }
        }
        for lang in c.toolchains.defaults.keys() {
            lang.parse::<embedide_toolchains::Language>()
                .map_err(Error)?;
        }
        if c.ai.max_tokens == 0 || c.ai.max_tokens > 64_000 {
            return Err(Error("ai.max_tokens must be 1..64000".into()));
        }
        Ok(c)
    }
}

/// A TOML file edited by dotted keys, preserving comments and layout.
pub struct ConfigFile {
    path: PathBuf,
    doc: toml_edit::DocumentMut,
}

impl ConfigFile {
    /// Open (an absent file is an empty document).
    pub fn open(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Error(format!("Cannot read {}: {e}", path.display()))),
        };
        let doc = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| Error(format!("{}: {e}", path.display())))?;
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    /// File path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn split(key: &str) -> Result<Vec<&str>> {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.iter().any(|p| {
            p.is_empty()
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_+".contains(c))
        }) {
            return Err(Error(format!(
                "Invalid key '{key}'; use dotted names like toolchain.c"
            )));
        }
        Ok(parts)
    }

    /// Value as TOML text, if set.
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let parts = Self::split(key)?;
        let mut item = self.doc.as_item();
        for p in parts {
            match item.get(p) {
                Some(i) => item = i,
                None => return Ok(None),
            }
        }
        Ok(Some(render(item)))
    }

    /// Set a value. `value` is parsed as TOML (`true`, `3`, `["-O2"]`) and
    /// otherwise stored as a string.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let parts = Self::split(key)?;
        if parts.iter().any(|p| crate::looks_secret(p)) {
            return Err(Error(format!(
                "'{key}' looks like a credential. Secrets are never written to configuration files; use `embed ai login` or an environment variable"
            )));
        }
        let parsed = format!("v = {value}")
            .parse::<toml_edit::DocumentMut>()
            .ok()
            .and_then(|d| d.get("v").and_then(|i| i.as_value().cloned()))
            .unwrap_or_else(|| toml_edit::Value::from(value));
        let (last, parents) = parts.split_last().expect("non-empty");
        let mut table = self.doc.as_table_mut();
        for p in parents {
            let entry = table.entry(p).or_insert_with(|| {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                toml_edit::Item::Table(t)
            });
            table = entry
                .as_table_mut()
                .ok_or_else(|| Error(format!("'{p}' in '{key}' is a value, not a table")))?;
        }
        table.insert(last, toml_edit::Item::Value(parsed));
        Ok(())
    }

    /// Remove a key; returns whether it existed.
    pub fn unset(&mut self, key: &str) -> Result<bool> {
        let parts = Self::split(key)?;
        let (last, parents) = parts.split_last().expect("non-empty");
        let mut table = self.doc.as_table_mut();
        for p in parents {
            match table.get_mut(p).and_then(|i| i.as_table_mut()) {
                Some(t) => table = t,
                None => return Ok(false),
            }
        }
        Ok(table.remove(last).is_some())
    }

    /// Every leaf as `key = value`.
    pub fn list(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        flatten("", self.doc.as_item(), &mut out);
        out
    }

    /// Document text.
    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// Validate with `check`, then write atomically.
    pub fn save(&self, check: impl Fn(&str) -> Result<()>) -> Result<()> {
        let text = self.doc.to_string();
        check(&text)?;
        write_atomic(&self.path, text.as_bytes(), false)
    }
}

fn render(item: &toml_edit::Item) -> String {
    match item {
        toml_edit::Item::Value(v) => v.to_string().trim().to_string(),
        other => other.to_string().trim().to_string(),
    }
}

fn flatten(prefix: &str, item: &toml_edit::Item, out: &mut Vec<(String, String)>) {
    match item {
        toml_edit::Item::Table(t) => {
            for (k, v) in t.iter() {
                let key = if prefix.is_empty() {
                    k.to_string()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        toml_edit::Item::Value(toml_edit::Value::InlineTable(t)) => {
            for (k, v) in t.iter() {
                out.push((format!("{prefix}.{k}"), v.to_string().trim().to_string()));
            }
        }
        toml_edit::Item::None => {}
        other => out.push((prefix.to_string(), render(other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("embedide-cfg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("config.toml")
    }

    #[test]
    fn set_get_unset_preserves_comments() {
        let p = tmp("edit");
        std::fs::write(&p, "# my notes\n[toolchains]\nsearch_dirs = []\n").unwrap();
        let mut f = ConfigFile::open(&p).unwrap();
        f.set("toolchains.defaults.c", "clang").unwrap();
        f.set("ai.max_tokens", "512").unwrap();
        f.set("toolchains.search_dirs", r#"["/opt/my tools/bin"]"#)
            .unwrap();
        f.save(|t| UserConfig::parse(t).map(|_| ())).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# my notes"));
        let c = UserConfig::parse(&text).unwrap();
        assert_eq!(c.toolchains.defaults["c"], "clang");
        assert_eq!(c.ai.max_tokens, 512);
        assert_eq!(
            c.toolchains.search_dirs,
            vec![PathBuf::from("/opt/my tools/bin")]
        );
        let f = ConfigFile::open(&p).unwrap();
        assert_eq!(
            f.get("toolchains.defaults.c").unwrap().as_deref(),
            Some("\"clang\"")
        );
        let mut f = ConfigFile::open(&p).unwrap();
        assert!(f.unset("ai.max_tokens").unwrap());
        assert!(!f.unset("ai.nothing").unwrap());
    }

    #[test]
    fn secrets_and_invalid_values_are_refused() {
        let p = tmp("secret");
        let mut f = ConfigFile::open(&p).unwrap();
        assert!(f
            .set("ai.providers.openai.api_key", "sk-x")
            .unwrap_err()
            .0
            .contains("credential"));
        f.set("ai.max_tokens", "0").unwrap();
        assert!(f.save(|t| UserConfig::parse(t).map(|_| ())).is_err());
        assert!(!p.exists(), "invalid configuration is not written");
        assert!(f.set("a..b", "1").is_err());
    }
}
