use std::path::PathBuf;
use std::time::Duration;

#[cfg(feature = "live-provider")]
use judge::{EndpointUrl, OpenAiCompatibleProviderClient};
use judge::{
    EnvironmentSecretResolver, FixtureProviderClient, ProviderAuthorization, ProviderClient,
    ProviderModelName, ProviderName, ResolvedProviderAuthorization, SecretSourceReference,
};
use mind_judge::{
    DEFAULT_LIVE_MODEL, MindJudgeAdapter, MindJudgeAdapterConfiguration,
    MindJudgeConfigurationPath, MindJudgeSocketPath, MindJudgeSocketServer,
};

const DEFAULT_IDLE_TIMEOUT_MILLISECONDS: u64 = 30_000;

#[tokio::main]
async fn main() {
    let result = match MindJudgeCommand::from_environment() {
        Ok(command) => command.run().await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("mind-judge: {error}");
        std::process::exit(1);
    }
}

#[derive(Debug)]
struct MindJudgeCommand {
    socket: PathBuf,
    config_root: Option<PathBuf>,
    provider: ProviderSelection,
    model: String,
    endpoint: Option<String>,
    bearer_secret_source: Option<String>,
    fixture_output: Option<String>,
    idle_timeout_milliseconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderSelection {
    Fixture,
    OpenAiCompatible,
}

impl MindJudgeCommand {
    fn from_environment() -> Result<Self, String> {
        let mut arguments = std::env::args().skip(1);
        let Some(command) = arguments.next() else {
            return Err(Self::usage());
        };
        if command != "serve" {
            return Err(Self::usage());
        }
        let mut socket = None;
        let mut config_root = None;
        let mut provider = std::env::var("MIND_JUDGE_PROVIDER")
            .ok()
            .map(|value| ProviderSelection::from_name(&value))
            .transpose()?;
        let mut model =
            std::env::var("MIND_JUDGE_MODEL").unwrap_or_else(|_| DEFAULT_LIVE_MODEL.to_owned());
        let mut endpoint = std::env::var("MIND_JUDGE_PROVIDER_ENDPOINT").ok();
        let mut bearer_secret_source = std::env::var("MIND_JUDGE_BEARER_SECRET_SOURCE").ok();
        let mut fixture_output = None;
        let mut idle_timeout_milliseconds = DEFAULT_IDLE_TIMEOUT_MILLISECONDS;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--socket" => socket = arguments.next().map(PathBuf::from),
                "--config-root" => config_root = arguments.next().map(PathBuf::from),
                "--provider" => {
                    provider = Some(ProviderSelection::from_name(
                        &arguments.next().ok_or_else(Self::usage)?,
                    )?)
                }
                "--endpoint" => endpoint = Some(arguments.next().ok_or_else(Self::usage)?),
                "--model" => model = arguments.next().ok_or_else(Self::usage)?,
                "--bearer-secret-source" => {
                    bearer_secret_source = Some(arguments.next().ok_or_else(Self::usage)?)
                }
                "--fixture-output" => {
                    fixture_output = Some(arguments.next().ok_or_else(Self::usage)?);
                    provider = Some(ProviderSelection::Fixture);
                }
                "--idle-timeout-ms" => {
                    idle_timeout_milliseconds = arguments
                        .next()
                        .ok_or_else(Self::usage)?
                        .parse()
                        .map_err(|_| Self::usage())?;
                }
                _ => return Err(Self::usage()),
            }
        }
        Ok(Self {
            socket: socket.ok_or_else(Self::usage)?,
            config_root,
            provider: provider.unwrap_or(ProviderSelection::OpenAiCompatible),
            model,
            endpoint,
            bearer_secret_source,
            fixture_output,
            idle_timeout_milliseconds,
        })
    }

    async fn run(self) -> Result<(), String> {
        let configuration_root =
            MindJudgeConfigurationPath::from_environment_or(self.config_root.clone())
                .map_err(|error| error.to_string())?;
        let provider_name = ProviderName::unchecked(self.provider.provider_name());
        let authorization = self.resolved_authorization()?;
        let client = self.provider_client()?;
        let adapter = MindJudgeAdapter::new(
            MindJudgeAdapterConfiguration::new(
                configuration_root,
                provider_name,
                ProviderModelName::unchecked(self.model),
                authorization,
            ),
            client,
        );
        MindJudgeSocketServer::new(MindJudgeSocketPath::new(self.socket), adapter)
            .serve_until_idle(Duration::from_millis(self.idle_timeout_milliseconds))
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn resolved_authorization(&self) -> Result<ResolvedProviderAuthorization, String> {
        let authorization = match &self.bearer_secret_source {
            Some(reference) => ProviderAuthorization::bearer_secret_source(
                SecretSourceReference::new(reference).map_err(|error| error.to_string())?,
            ),
            None => ProviderAuthorization::no_secret(),
        };
        authorization
            .resolve(&EnvironmentSecretResolver)
            .map_err(|error| error.to_string())
    }

    fn provider_client(&self) -> Result<Box<dyn ProviderClient>, String> {
        match self.provider {
            ProviderSelection::Fixture => Ok(Box::new(FixtureProviderClient::from_text(
                self.fixture_output
                    .clone()
                    .unwrap_or_else(|| "(Accept None)".to_owned()),
            ))),
            ProviderSelection::OpenAiCompatible => self.openai_compatible_client(),
        }
    }

    #[cfg(feature = "live-provider")]
    fn openai_compatible_client(&self) -> Result<Box<dyn ProviderClient>, String> {
        let endpoint = self.endpoint.clone().ok_or_else(|| {
            "openai-compatible provider requires --endpoint or MIND_JUDGE_PROVIDER_ENDPOINT"
                .to_owned()
        })?;
        Ok(Box::new(OpenAiCompatibleProviderClient::new(
            EndpointUrl::new(endpoint).map_err(|error| error.to_string())?,
        )))
    }

    #[cfg(not(feature = "live-provider"))]
    fn openai_compatible_client(&self) -> Result<Box<dyn ProviderClient>, String> {
        let _endpoint = &self.endpoint;
        Err("openai-compatible provider requires the live-provider feature".to_owned())
    }

    fn usage() -> String {
        "usage: mind-judge serve --socket <path> [--config-root <path>] [--provider fixture|openai-compatible] [--endpoint <url>] [--model <name>] [--bearer-secret-source env:NAME] [--fixture-output <nota>] [--idle-timeout-ms <milliseconds>]".to_owned()
    }
}

impl ProviderSelection {
    fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "fixture" => Ok(Self::Fixture),
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            _ => Err(MindJudgeCommand::usage()),
        }
    }

    fn provider_name(&self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}
