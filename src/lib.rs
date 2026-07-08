//! Mind-specific judge adapter scaffold.
//!
//! This crate is the edge adapter: it consumes the `signal-mind-judge` contract,
//! reads prompt/config data from a configured `mind-judge-config` path, and will
//! call providers through `judge`. It is not a Mind daemon core.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use thiserror::Error;

pub type AdapterRequest = signal_mind_judge::MindJudgeRequest;
pub type AdapterReply = signal_mind_judge::MindJudgeReply;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("mind judge config path is empty")]
    EmptyConfigurationPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MindJudgeConfigurationPath(PathBuf);

impl MindJudgeConfigurationPath {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(Error::EmptyConfigurationPath);
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MindJudgeAdapterConfiguration {
    configuration_path: MindJudgeConfigurationPath,
    provider_name: judge::ProviderName,
    model_name: judge::ProviderModelName,
    secret_source_reference: judge::SecretSourceReference,
}

impl MindJudgeAdapterConfiguration {
    pub fn new(
        configuration_path: MindJudgeConfigurationPath,
        provider_name: judge::ProviderName,
        model_name: judge::ProviderModelName,
        secret_source_reference: judge::SecretSourceReference,
    ) -> Self {
        Self {
            configuration_path,
            provider_name,
            model_name,
            secret_source_reference,
        }
    }

    pub fn configuration_path(&self) -> &MindJudgeConfigurationPath {
        &self.configuration_path
    }

    pub fn provider_name(&self) -> &judge::ProviderName {
        &self.provider_name
    }

    pub fn model_name(&self) -> &judge::ProviderModelName {
        &self.model_name
    }

    pub fn secret_source_reference(&self) -> &judge::SecretSourceReference {
        &self.secret_source_reference
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MindJudgeAdapter {
    configuration: MindJudgeAdapterConfiguration,
}

impl MindJudgeAdapter {
    pub fn new(configuration: MindJudgeAdapterConfiguration) -> Self {
        Self { configuration }
    }

    pub fn configuration(&self) -> &MindJudgeAdapterConfiguration {
        &self.configuration
    }
}
