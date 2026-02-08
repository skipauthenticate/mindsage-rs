//! LLM configuration persistence and provider selection.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::types::{LLMConfigResponse, LLMConfigUpdate, LLMProvider};

pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";
pub const DEFAULT_GROQ_MODEL: &str = "llama-3.3-70b-versatile";

pub const OPENAI_MODELS: &[&str] = &["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "gpt-3.5-turbo"];
pub const ANTHROPIC_MODELS: &[&str] = &[
    "claude-sonnet-4-20250514",
    "claude-3-5-sonnet-20241022",
    "claude-3-5-haiku-20241022",
];
pub const GROQ_MODELS: &[&str] = &[
    "llama-3.3-70b-versatile",
    "llama-3.1-8b-instant",
    "mixtral-8x7b-32768",
    "gemma2-9b-it",
];

/// Stored LLM configuration (persisted to llm-config.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    #[serde(default = "default_preferred")]
    pub preferred_provider: String,
    #[serde(default)]
    pub openai_api_key: Option<String>,
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
    #[serde(default)]
    pub groq_api_key: Option<String>,
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    #[serde(default = "default_anthropic_model")]
    pub anthropic_model: String,
    #[serde(default = "default_groq_model")]
    pub groq_model: String,
    /// Path to config file for saving.
    #[serde(skip)]
    pub config_path: PathBuf,
}

fn default_preferred() -> String {
    "auto".into()
}
fn default_openai_model() -> String {
    DEFAULT_OPENAI_MODEL.into()
}
fn default_anthropic_model() -> String {
    DEFAULT_ANTHROPIC_MODEL.into()
}
fn default_groq_model() -> String {
    DEFAULT_GROQ_MODEL.into()
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            preferred_provider: "auto".into(),
            openai_api_key: None,
            anthropic_api_key: None,
            groq_api_key: None,
            openai_model: DEFAULT_OPENAI_MODEL.into(),
            anthropic_model: DEFAULT_ANTHROPIC_MODEL.into(),
            groq_model: DEFAULT_GROQ_MODEL.into(),
            config_path: PathBuf::new(),
        }
    }
}

impl LLMConfig {
    /// Load config from file, falling back to env vars and defaults.
    pub fn load(config_path: &Path) -> Self {
        let mut config: LLMConfig = std::fs::read_to_string(config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        config.config_path = config_path.to_path_buf();

        // Env vars as fallback for API keys
        if config.openai_api_key.is_none() {
            config.openai_api_key = std::env::var("OPENAI_API_KEY").ok();
        }
        if config.anthropic_api_key.is_none() {
            config.anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        }
        if config.groq_api_key.is_none() {
            config.groq_api_key = std::env::var("GROQ_API_KEY").ok();
        }

        config
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<(), std::io::Error> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e)
        })?;
        std::fs::write(&self.config_path, json)?;
        info!("Saved LLM config to {}", self.config_path.display());
        Ok(())
    }

    /// Apply an update, merging with existing config.
    pub fn apply_update(&mut self, update: &LLMConfigUpdate) {
        if let Some(p) = &update.preferred_provider {
            self.preferred_provider = p.clone();
        }
        if let Some(k) = &update.openai_api_key {
            self.openai_api_key = Some(k.clone());
        }
        if let Some(k) = &update.anthropic_api_key {
            self.anthropic_api_key = Some(k.clone());
        }
        if let Some(k) = &update.groq_api_key {
            self.groq_api_key = Some(k.clone());
        }
        if let Some(m) = &update.openai_model {
            self.openai_model = m.clone();
        }
        if let Some(m) = &update.anthropic_model {
            self.anthropic_model = m.clone();
        }
        if let Some(m) = &update.groq_model {
            self.groq_model = m.clone();
        }
    }

    /// Resolve which provider and model to use.
    pub fn resolve_provider(&self) -> Option<(LLMProvider, String, String)> {
        // Explicit preference
        if self.preferred_provider != "auto" {
            return match self.preferred_provider.as_str() {
                "openai" => self
                    .openai_api_key
                    .as_ref()
                    .map(|k| (LLMProvider::OpenAI, self.openai_model.clone(), k.clone())),
                "anthropic" => self
                    .anthropic_api_key
                    .as_ref()
                    .map(|k| (LLMProvider::Anthropic, self.anthropic_model.clone(), k.clone())),
                "groq" => self
                    .groq_api_key
                    .as_ref()
                    .map(|k| (LLMProvider::Groq, self.groq_model.clone(), k.clone())),
                _ => None,
            };
        }

        // Auto mode: Anthropic > Groq > OpenAI
        if let Some(k) = &self.anthropic_api_key {
            return Some((LLMProvider::Anthropic, self.anthropic_model.clone(), k.clone()));
        }
        if let Some(k) = &self.groq_api_key {
            return Some((LLMProvider::Groq, self.groq_model.clone(), k.clone()));
        }
        if let Some(k) = &self.openai_api_key {
            return Some((LLMProvider::OpenAI, self.openai_model.clone(), k.clone()));
        }

        None
    }

    /// Build the public config response (no API keys exposed).
    pub fn to_response(&self) -> LLMConfigResponse {
        let resolved = self.resolve_provider();
        LLMConfigResponse {
            preferred_provider: self.preferred_provider.clone(),
            openai_configured: self.openai_api_key.is_some(),
            anthropic_configured: self.anthropic_api_key.is_some(),
            groq_configured: self.groq_api_key.is_some(),
            openai_model: self.openai_model.clone(),
            anthropic_model: self.anthropic_model.clone(),
            groq_model: self.groq_model.clone(),
            active_provider: resolved.map(|(p, _, _)| p.to_string()),
        }
    }

    /// Get available models for the active provider.
    pub fn available_models(&self) -> Vec<String> {
        match self.resolve_provider() {
            Some((LLMProvider::OpenAI, _, _)) => OPENAI_MODELS.iter().map(|s| s.to_string()).collect(),
            Some((LLMProvider::Anthropic, _, _)) => {
                ANTHROPIC_MODELS.iter().map(|s| s.to_string()).collect()
            }
            Some((LLMProvider::Groq, _, _)) => GROQ_MODELS.iter().map(|s| s.to_string()).collect(),
            None => Vec::new(),
        }
    }
}
