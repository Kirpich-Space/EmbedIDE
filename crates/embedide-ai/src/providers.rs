//! The four initial provider families.
//!
//! Endpoints follow the providers' public API documentation:
//! - Anthropic Messages API: `POST {base}/v1/messages`, `x-api-key`,
//!   `anthropic-version: 2023-06-01`, SSE streaming; `GET {base}/v1/models`.
//! - OpenAI, Moonshot (Kimi) and xAI (Grok) Chat Completions:
//!   `POST {base}/chat/completions` with a Bearer key, SSE streaming;
//!   `GET {base}/models`. Moonshot and xAI document OpenAI compatibility.
use serde::Serialize;
use std::{fmt, str::FromStr};

/// Provider family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// Anthropic — Claude.
    Anthropic,
    /// OpenAI — GPT / Codex.
    Openai,
    /// Moonshot AI — Kimi.
    Moonshot,
    /// xAI — Grok.
    Xai,
    /// Google Antigravity CLI (`agy`).
    Antigravity,
}

/// Wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    /// Anthropic Messages API.
    AnthropicMessages,
    /// OpenAI-style Chat Completions.
    ChatCompletions,
}

/// Official subscription integration, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subscription {
    /// Claude Code CLI in print mode (`claude -p`).
    ClaudeCode,
    /// Codex CLI non-interactive mode (`codex exec`).
    Codex,
    /// Antigravity CLI non-interactive mode (`agy --print`).
    Antigravity,
    /// No integration in EmbedIDE (with the reason).
    NotIntegrated(&'static str),
}

/// Static facts about a provider.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    /// Id used in configuration.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Model family name, for display only.
    pub models_family: &'static str,
    /// Default API base URL.
    pub base_url: &'static str,
    /// Environment variable holding the API key.
    pub key_env: &'static str,
    /// Wire protocol.
    pub protocol: Protocol,
    /// Subscription support.
    pub subscription: Subscription,
}

impl ProviderId {
    /// All providers.
    pub const ALL: [ProviderId; 5] = [
        Self::Anthropic,
        Self::Openai,
        Self::Moonshot,
        Self::Xai,
        Self::Antigravity,
    ];

    /// Configuration id.
    pub fn id(self) -> &'static str {
        self.info().id
    }

    /// Static facts.
    pub fn info(self) -> ProviderInfo {
        match self {
            Self::Anthropic => ProviderInfo {
                id: "anthropic", name: "Anthropic", models_family: "Claude", base_url: "https://api.anthropic.com",
                key_env: "ANTHROPIC_API_KEY", protocol: Protocol::AnthropicMessages, subscription: Subscription::ClaudeCode,
            },
            Self::Openai => ProviderInfo {
                id: "openai", name: "OpenAI", models_family: "GPT / Codex", base_url: "https://api.openai.com/v1",
                key_env: "OPENAI_API_KEY", protocol: Protocol::ChatCompletions, subscription: Subscription::Codex,
            },
            Self::Moonshot => ProviderInfo {
                id: "moonshot", name: "Moonshot AI", models_family: "Kimi", base_url: "https://api.moonshot.ai/v1",
                key_env: "MOONSHOT_API_KEY", protocol: Protocol::ChatCompletions,
                subscription: Subscription::NotIntegrated("Kimi Code CLI exists, but its non-interactive interface is not verified by EmbedIDE yet; use an API key"),
            },
            Self::Xai => ProviderInfo {
                id: "xai", name: "xAI", models_family: "Grok", base_url: "https://api.x.ai/v1",
                key_env: "XAI_API_KEY", protocol: Protocol::ChatCompletions,
                subscription: Subscription::NotIntegrated("no officially documented subscription CLI interface verified; use an API key"),
            },
            Self::Antigravity => ProviderInfo {
                id: "antigravity", name: "Antigravity", models_family: "Gemini / Claude", base_url: "",
                key_env: "", protocol: Protocol::ChatCompletions,
                subscription: Subscription::Antigravity,
            },
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for ProviderId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Ok(Self::Anthropic),
            "openai" | "gpt" | "codex" => Ok(Self::Openai),
            "moonshot" | "kimi" => Ok(Self::Moonshot),
            "xai" | "grok" => Ok(Self::Xai),
            "antigravity" | "agy" => Ok(Self::Antigravity),
            _ => Err(format!(
                "Unknown AI provider '{s}'; expected anthropic, openai, moonshot, xai or antigravity"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_ids() {
        for p in ProviderId::ALL {
            assert_eq!(p.id().parse::<ProviderId>().unwrap(), p);
        }
        assert_eq!("kimi".parse::<ProviderId>().unwrap(), ProviderId::Moonshot);
        assert_eq!("grok".parse::<ProviderId>().unwrap(), ProviderId::Xai);
        assert!("gemini".parse::<ProviderId>().is_err());
    }

    #[test]
    fn only_documented_subscription_clis_are_integrated() {
        assert_eq!(
            ProviderId::Anthropic.info().subscription,
            Subscription::ClaudeCode
        );
        assert_eq!(ProviderId::Openai.info().subscription, Subscription::Codex);
        assert!(matches!(
            ProviderId::Moonshot.info().subscription,
            Subscription::NotIntegrated(_)
        ));
        assert!(matches!(
            ProviderId::Xai.info().subscription,
            Subscription::NotIntegrated(_)
        ));
    }
}
