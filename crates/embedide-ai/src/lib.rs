//! Provider-independent AI layer for EmbedIDE.
//!
//! Provider ≠ model ≠ authentication method ≠ IDE feature:
//! - [`providers`] knows the four provider families and their wire protocols;
//! - models are always chosen by the user (flag or configuration), never
//!   invented here; [`Backend::models`] lists what a provider offers;
//! - [`auth`] resolves API keys (environment or a private credentials file)
//!   and the officially supported subscription CLIs;
//! - features ([`chat`], [`complete`], [`edit`]) only talk to the
//!   [`Backend`] trait, so the future Qt editor reuses them unchanged.
//!
//! Network requests happen only when a caller invokes a feature explicitly.
//! `EMBEDIDE_AI_OFFLINE=1` refuses every request (used by tests).

pub mod auth;
pub mod chat;
pub mod complete;
pub mod context;
pub mod edit;
pub mod providers;
mod subscription;
mod wire;

use serde::Serialize;
use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub use auth::{AuthMode, Secret};
pub use providers::{ProviderId, ProviderInfo};

/// AI error. Messages never contain credentials (see [`Secret`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Missing or invalid configuration (no model, no key, unsupported mode).
    Config(String),
    /// The provider rejected the credentials (401/403).
    Auth(String),
    /// Rate limited (429); seconds to wait when the provider said so.
    RateLimited(Option<u64>, String),
    /// Any other HTTP or provider error.
    Provider(u16, String),
    /// Transport problem (DNS, TLS, timeout, connection reset).
    Network(String),
    /// Cancelled by the caller.
    Cancelled,
    /// Network use is disabled (EMBEDIDE_AI_OFFLINE=1).
    Offline,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(m) => write!(f, "{m}"),
            Self::Auth(m) => write!(f, "Authentication failed: {m}"),
            Self::RateLimited(Some(s), m) => write!(f, "Rate limited; retry after {s} s: {m}"),
            Self::RateLimited(None, m) => write!(f, "Rate limited: {m}"),
            Self::Provider(code, m) => write!(f, "Provider error {code}: {m}"),
            Self::Network(m) => write!(f, "Network error: {m}"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::Offline => write!(f, "AI network access is disabled (EMBEDIDE_AI_OFFLINE=1)"),
        }
    }
}
impl std::error::Error for Error {}

/// Result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Cooperative cancellation shared between a request and its caller.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);
impl Cancel {
    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release)
    }
    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Chat role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The person.
    User,
    /// The model.
    Assistant,
}

/// One message.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    /// Who said it.
    pub role: Role,
    /// Text.
    pub content: String,
}

impl Message {
    /// A user message.
    pub fn user(s: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: s.into(),
        }
    }
    /// An assistant message.
    pub fn assistant(s: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: s.into(),
        }
    }
}

/// Provider-independent request.
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    /// Model id, exactly as the provider names it.
    pub model: String,
    /// System instructions.
    pub system: Option<String>,
    /// Conversation.
    pub messages: Vec<Message>,
    /// Output token cap.
    pub max_tokens: u32,
    /// Sampling temperature, when set.
    pub temperature: Option<f32>,
    /// Stream deltas when the backend supports it.
    pub stream: bool,
}

/// Token usage reported by the provider (absent fields are unknown).
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Usage {
    /// Input tokens.
    pub input_tokens: Option<u64>,
    /// Output tokens.
    pub output_tokens: Option<u64>,
}

/// Provider-independent response.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    /// Full text.
    pub text: String,
    /// Usage, if reported.
    pub usage: Usage,
    /// Why generation stopped, if reported.
    pub stop_reason: Option<String>,
    /// Model id the provider reports it used.
    pub model: Option<String>,
}

/// A model a provider offers.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    /// Model id.
    pub id: String,
    /// Display name, when provided.
    pub name: Option<String>,
}

/// Anything that answers requests: an HTTP API adapter, a subscription CLI,
/// or a test double.
pub trait Backend: Send + Sync {
    /// Provider and auth mode label for display.
    fn label(&self) -> String;
    /// Send one request; `on_delta` receives streamed text.
    fn send(
        &self,
        req: &Request,
        cancel: &Cancel,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Response>;
    /// List models (network).
    fn models(&self) -> Result<Vec<ModelInfo>>;
}

pub(crate) fn offline() -> bool {
    std::env::var("EMBEDIDE_AI_OFFLINE").is_ok_and(|v| v == "1")
}

/// Build the backend for a provider and auth mode from configuration.
pub fn backend(
    provider: ProviderId,
    mode: AuthMode,
    cfg: &embedide_workspace::config::AiConfig,
) -> Result<Box<dyn Backend>> {
    let pc = cfg
        .providers
        .get(provider.id())
        .cloned()
        .unwrap_or_default();
    match mode {
        AuthMode::ApiKey => {
            let key = auth::api_key(provider)?.ok_or_else(|| {
                Error::Config(format!(
                    "No API key for {}. Set {} or run `embed ai login {}` (the key is stored outside your projects)",
                    provider.info().name,
                    provider.info().key_env,
                    provider.id()
                ))
            })?;
            let base = pc
                .base_url
                .clone()
                .unwrap_or_else(|| provider.info().base_url.to_string());
            Ok(Box::new(wire::HttpBackend::new(provider, base, key)?))
        }
        AuthMode::Subscription => subscription::backend(provider, pc.cli.clone()),
    }
}

/// An API-key backend for an explicit base URL (embedding front ends, tests).
/// Plain HTTP is accepted only for localhost.
pub fn api_backend(provider: ProviderId, base_url: &str, key: Secret) -> Result<Box<dyn Backend>> {
    Ok(Box::new(wire::HttpBackend::new(
        provider,
        base_url.to_string(),
        key,
    )?))
}
