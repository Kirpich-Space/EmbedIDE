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
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home().map(|h| h.join(".config")))
        .map(|c| c.join("embedide"));
    dir.ok_or_else(|| {
        Error("Cannot determine the configuration directory; set EMBEDIDE_CONFIG_DIR".into())
    })
}

/// `[toolchains]`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ToolchainsConfig {
    /// Extra directories searched after PATH.
    pub search_dirs: Vec<PathBuf>,
    /// User-defined toolchains (any path, any kind).
    pub custom: Vec<CustomToolchain>,
    /// Default toolchain id per language for projects that do not choose one.
    pub defaults: BTreeMap<String, String>,
    /// Names of downloadable toolchains (`[toolchains.install.<name>]` or built in) that must exist on
    /// this machine; missing ones are downloaded when `auto_install` is on.
    pub require: Vec<String>,
    /// Downloadable toolchains: name -> pinned archive (https URL + SHA-256). Only entries written here
    /// (or built in) are ever downloaded; a project manifest can only refer to them by name.
    pub install: BTreeMap<String, embedide_toolchains::InstallSpec>,
    /// Download missing required toolchains without asking (default true). `EMBEDIDE_OFFLINE=1` always wins.
    pub auto_install: bool,
}

impl Default for ToolchainsConfig {
    fn default() -> Self {
        Self {
            search_dirs: Vec::new(),
            custom: Vec::new(),
            defaults: BTreeMap::new(),
            require: Vec::new(),
            install: BTreeMap::new(),
            auto_install: true,
        }
    }
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

/// `[ai.cli]`: how the official Claude Code / Codex CLIs are run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct CliConfig {
    /// Longest a single CLI request may run, in seconds.
    pub timeout_secs: u64,
    /// Output accepted from one CLI request, in bytes.
    pub max_output_bytes: usize,
    /// Pass ANTHROPIC_API_KEY / OPENAI_API_KEY / CODEX_API_KEY through to the
    /// CLI. Off by default so a subscription request never becomes a billed
    /// API request because a key happens to be in the environment.
    pub allow_api_key: bool,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 600,
            max_output_bytes: 8 * 1024 * 1024,
            allow_api_key: false,
        }
    }
}

/// `[ai.agent]`: autonomous agent runs through the official CLIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct AgentConfig {
    /// `read-only`, `workspace-write` (default) or `approval-required`.
    pub policy: String,
    /// What plain text typed in the AI panel does with an agent-capable CLI
    /// backend: `agent` (default: run it as a task in the project) or `chat`
    /// (read-only answers). API backends always chat.
    pub mode: String,
    /// Longest one agent turn may run, in seconds.
    pub timeout_secs: u64,
    /// Bytes of workspace text files snapshotted before a turn (for review
    /// and `/revert`); larger workspaces are not snapshotted.
    pub snapshot_max_bytes: u64,
    /// Files snapshotted before a turn.
    pub snapshot_max_files: usize,
    /// Rounds of `/fix` (build, send errors, rebuild) before giving up.
    pub max_fix_iterations: u32,
    /// Command `/fix` runs when none is given (default: the project build).
    pub fix_command: Option<String>,
    /// Extra command names allowed without confirmation under
    /// `workspace-write` (Claude Code; added to the built-in development list).
    pub allow_commands: Vec<String>,
    /// Keep CLI sessions so a task can continue (`claude --resume`,
    /// `codex exec resume`). Off: every turn starts fresh.
    pub persist_sessions: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            policy: "workspace-write".into(),
            mode: "agent".into(),
            timeout_secs: 1800,
            snapshot_max_bytes: 64 * 1024 * 1024,
            snapshot_max_files: 20_000,
            max_fix_iterations: 3,
            fix_command: None,
            allow_commands: Vec::new(),
            persist_sessions: true,
        }
    }
}

/// `[ai.completion]`: inline AI suggestions in the editor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct CompletionConfig {
    /// `off`, `manual` (default: only on Alt+\ or :complete) or `auto`
    /// (debounced while typing). `ai.inline = true` means `auto`.
    pub mode: Option<String>,
    /// Backend for suggestions when it should differ from the chat backend
    /// (`codex-cli`, `claude-code`, `anthropic`, …).
    pub provider: Option<String>,
    /// Model for suggestions.
    pub model: Option<String>,
    /// Longest one suggestion request may take, in seconds.
    pub timeout_secs: u64,
    /// Characters before the cursor sent.
    pub prefix_chars: usize,
    /// Characters after the cursor sent.
    pub suffix_chars: usize,
    /// Suggestions remembered (same context → no new request).
    pub cache_entries: usize,
    /// Longest suggestion kept, in lines.
    pub max_lines: usize,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            mode: None,
            provider: None,
            model: None,
            timeout_secs: 60,
            prefix_chars: 4000,
            suffix_chars: 1500,
            cache_entries: 64,
            max_lines: 40,
        }
    }
}

/// `[ai.skills]`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct SkillsConfig {
    /// Use skills at all.
    pub enabled: bool,
    /// Extra skill directories, searched between the project and the
    /// global directory.
    pub dirs: Vec<PathBuf>,
    /// Largest SKILL.md loaded, in bytes.
    pub max_bytes: usize,
    /// Give agents EmbedIDE's built-in working instructions and the skills
    /// that match the project (C/C++, Rust, Zig, assembly, embedded)
    /// without `/skill`.
    pub auto: bool,
    /// Skills never activated automatically.
    pub exclude: Vec<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dirs: Vec::new(),
            max_bytes: 64 * 1024,
            auto: true,
            exclude: Vec::new(),
        }
    }
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
    /// Official CLI backends (`claude-code`, `codex-cli`).
    pub cli: CliConfig,
    /// Agent runs.
    pub agent: AgentConfig,
    /// Inline suggestions.
    pub completion: CompletionConfig,
    /// Skills.
    pub skills: SkillsConfig,
}

impl AiConfig {
    /// Effective inline suggestion mode: `off`, `manual` or `auto`.
    pub fn completion_mode(&self) -> &str {
        match self.completion.mode.as_deref() {
            Some(m) => m,
            None if self.inline => "auto",
            None => "manual",
        }
    }
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
            cli: CliConfig::default(),
            agent: AgentConfig::default(),
            completion: CompletionConfig::default(),
            skills: SkillsConfig::default(),
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
        for (name, spec) in &c.toolchains.install {
            embedide_toolchains::install::validate_spec(name, spec).map_err(Error)?;
        }
        for name in &c.toolchains.require {
            embedide_toolchains::install::validate_name(name).map_err(Error)?;
        }
        for lang in c.toolchains.defaults.keys() {
            lang.parse::<embedide_toolchains::Language>()
                .map_err(Error)?;
        }
        if c.ai.max_tokens == 0 || c.ai.max_tokens > 64_000 {
            return Err(Error("ai.max_tokens must be 1..64000".into()));
        }
        if !["read-only", "workspace-write", "approval-required"]
            .contains(&c.ai.agent.policy.as_str())
        {
            return Err(Error(format!(
                "ai.agent.policy '{}': use read-only, workspace-write or approval-required",
                c.ai.agent.policy
            )));
        }
        if !["chat", "agent"].contains(&c.ai.agent.mode.as_str()) {
            return Err(Error(format!(
                "ai.agent.mode '{}': use chat or agent",
                c.ai.agent.mode
            )));
        }
        if !["off", "manual", "auto"].contains(&c.ai.completion_mode()) {
            return Err(Error(format!(
                "ai.completion.mode '{}': use off, manual or auto",
                c.ai.completion_mode()
            )));
        }
        if c.ai.agent.timeout_secs == 0 || c.ai.completion.timeout_secs == 0 {
            return Err(Error(
                "ai.agent.timeout_secs and ai.completion.timeout_secs must be positive".into(),
            ));
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

    #[test]
    fn agent_completion_and_skill_settings_parse_with_defaults() {
        let c = UserConfig::parse("").unwrap();
        assert_eq!(c.ai.agent.policy, "workspace-write");
        assert_eq!(c.ai.completion_mode(), "manual");
        assert!(c.ai.skills.enabled);
        let c = UserConfig::parse("[ai]\ninline = true\n").unwrap();
        assert_eq!(c.ai.completion_mode(), "auto", "the old switch still works");
        let c = UserConfig::parse(
            "[ai.agent]\npolicy = \"approval-required\"\nallow_commands = [\"just\"]\n\
             [ai.completion]\nmode = \"off\"\nprovider = \"codex-cli\"\n\
             [ai.skills]\ndirs = [\"/opt/skills\"]\n",
        )
        .unwrap();
        assert_eq!(c.ai.agent.policy, "approval-required");
        assert_eq!(c.ai.agent.allow_commands, ["just"]);
        assert_eq!(c.ai.completion_mode(), "off");
        assert_eq!(c.ai.completion.provider.as_deref(), Some("codex-cli"));
        assert_eq!(c.ai.skills.dirs, [PathBuf::from("/opt/skills")]);
        assert!(UserConfig::parse("[ai.agent]\npolicy = \"yolo\"\n").is_err());
        assert!(UserConfig::parse("[ai.completion]\nmode = \"always\"\n").is_err());
        assert!(UserConfig::parse("[ai.agent]\nunknown = 1\n").is_err());
    }
}
