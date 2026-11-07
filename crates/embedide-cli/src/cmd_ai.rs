//! `embedide ai …` (filled in by the AI milestone)
use crate::{CliResult, Out};
use clap::Subcommand;
use serde_json::Value;

#[derive(Subcommand)]
pub enum AiCmd {
    /// List AI providers.
    Providers,
}

pub fn run(_cmd: AiCmd, _out: &Out) -> CliResult<(Value, u8)> {
    Err(crate::fail("AI is not available in this build".into()))
}
