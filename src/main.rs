use std::path::PathBuf;

use judge::FixtureProviderClient;
use mind_judge::{
    MindJudgeAdapter, MindJudgeAdapterConfiguration, MindJudgeConfigurationPath,
    MindJudgeSocketPath, MindJudgeSocketServer,
};

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
    fixture_output: String,
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
        let mut fixture_output = "(Accept None)".to_owned();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--socket" => socket = arguments.next().map(PathBuf::from),
                "--config-root" => config_root = arguments.next().map(PathBuf::from),
                "--fixture-output" => {
                    fixture_output = arguments.next().ok_or_else(Self::usage)?;
                }
                _ => return Err(Self::usage()),
            }
        }
        Ok(Self {
            socket: socket.ok_or_else(Self::usage)?,
            config_root,
            fixture_output,
        })
    }

    async fn run(self) -> Result<(), String> {
        let configuration_root = MindJudgeConfigurationPath::from_environment_or(self.config_root)
            .map_err(|error| error.to_string())?;
        let adapter = MindJudgeAdapter::new(
            MindJudgeAdapterConfiguration::fixture(configuration_root),
            FixtureProviderClient::from_text(self.fixture_output),
        );
        MindJudgeSocketServer::new(MindJudgeSocketPath::new(self.socket), adapter)
            .serve_one()
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn usage() -> String {
        "usage: mind-judge serve --socket <path> [--config-root <path>] [--fixture-output <nota>]"
            .to_owned()
    }
}
