//! Mind-specific judge adapter.
//!
//! `mind-judge` is the text/model edge: it renders the typed
//! `signal-mind-judge` request as NOTA for the model, calls a provider through
//! `judge`, parses the provider text back into the typed reply contract, and can
//! serve the contract over a Unix socket.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use judge::{
    ProviderCallReply, ProviderCallRequest, ProviderClient, ProviderMessage, ProviderModelName,
    ProviderName, ResolvedProviderAuthorization,
};
use nota::{NotaEncode, NotaSource};
use signal_mind_judge::{
    KnowledgeJudgePacket, KnowledgeJudgeResponse, MindJudgeFrame, MindJudgeFrameCodec,
    MindJudgeFrameCodecError, MindJudgeReply, MindJudgeRequest, MindJudgeRequestRejection,
    MindJudgeRequestRejectionReason, TextBody,
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::timeout;

pub const DEFAULT_LIVE_MODEL: &str = "chatgpt-5.4-mini";
const ACCEPTED_KNOWLEDGE_PROMPT_PATH: &str = "prompts/accepted-knowledge/system.md";

pub type AdapterRequest = MindJudgeRequest;
pub type AdapterReply = MindJudgeReply;

#[derive(Debug, Error)]
pub enum Error {
    #[error("mind judge config path is empty")]
    EmptyConfigurationPath,

    #[error("read prompt: {0}")]
    ReadPrompt(std::io::Error),

    #[error("provider call failed: {0}")]
    Provider(#[from] judge::Error),

    #[error("response parse failed: {0}")]
    ResponseParse(String),

    #[error("socket unavailable: {0}")]
    Socket(std::io::Error),

    #[error("frame failed: {0}")]
    Frame(String),

    #[error("unexpected frame: {0}")]
    UnexpectedFrame(&'static str),
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

    pub fn from_environment_or(path: Option<PathBuf>) -> Result<Self, Error> {
        match path {
            Some(path) => Self::new(path),
            None => Self::new(
                std::env::var_os("MIND_JUDGE_CONFIG")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(".")),
            ),
        }
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MindJudgeAdapterConfiguration {
    configuration_path: MindJudgeConfigurationPath,
    provider_name: ProviderName,
    model_name: ProviderModelName,
    authorization: ResolvedProviderAuthorization,
}

impl MindJudgeAdapterConfiguration {
    pub fn new(
        configuration_path: MindJudgeConfigurationPath,
        provider_name: ProviderName,
        model_name: ProviderModelName,
        authorization: ResolvedProviderAuthorization,
    ) -> Self {
        Self {
            configuration_path,
            provider_name,
            model_name,
            authorization,
        }
    }

    pub fn fixture(configuration_path: MindJudgeConfigurationPath) -> Self {
        Self::new(
            configuration_path,
            ProviderName::unchecked("fixture"),
            ProviderModelName::unchecked(DEFAULT_LIVE_MODEL),
            ResolvedProviderAuthorization::no_secret(),
        )
    }

    pub fn configuration_path(&self) -> &MindJudgeConfigurationPath {
        &self.configuration_path
    }

    pub fn provider_name(&self) -> &ProviderName {
        &self.provider_name
    }

    pub fn model_name(&self) -> &ProviderModelName {
        &self.model_name
    }

    pub fn authorization(&self) -> &ResolvedProviderAuthorization {
        &self.authorization
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedKnowledgePrompt {
    system_text: String,
}

impl AcceptedKnowledgePrompt {
    pub fn read_from(configuration_path: &MindJudgeConfigurationPath) -> Result<Self, Error> {
        let path = configuration_path
            .as_path()
            .join(ACCEPTED_KNOWLEDGE_PROMPT_PATH);
        let system_text = std::fs::read_to_string(path).map_err(Error::ReadPrompt)?;
        Ok(Self { system_text })
    }

    pub fn system_text(&self) -> &str {
        self.system_text.as_str()
    }
}

pub struct MindJudgeAdapter<Client> {
    configuration: MindJudgeAdapterConfiguration,
    provider_client: Client,
}

impl<Client> MindJudgeAdapter<Client> {
    pub fn new(configuration: MindJudgeAdapterConfiguration, provider_client: Client) -> Self {
        Self {
            configuration,
            provider_client,
        }
    }

    pub fn configuration(&self) -> &MindJudgeAdapterConfiguration {
        &self.configuration
    }
}

impl<Client> MindJudgeAdapter<Client>
where
    Client: ProviderClient,
{
    pub fn judge(&self, request: MindJudgeRequest) -> MindJudgeReply {
        match request {
            MindJudgeRequest::JudgeKnowledge(packet) => self.judge_knowledge(packet),
        }
    }

    fn judge_knowledge(&self, packet: KnowledgeJudgePacket) -> MindJudgeReply {
        let prompt =
            match AcceptedKnowledgePrompt::read_from(self.configuration.configuration_path()) {
                Ok(prompt) => prompt,
                Err(error) => {
                    return MindJudgeReply::RequestRejected(MindJudgeRequestRejection::new(
                        MindJudgeRequestRejectionReason::ConfigurationUnavailable,
                        Self::message(error.to_string()),
                    ));
                }
            };
        let request = self.provider_request(&prompt, &packet);
        let reply = match self.provider_client.call(request) {
            Ok(reply) => reply,
            Err(error) => {
                return MindJudgeReply::RequestRejected(MindJudgeRequestRejection::new(
                    MindJudgeRequestRejectionReason::ProviderUnavailable,
                    Self::message(error.to_string()),
                ));
            }
        };
        self.reply_from_provider(reply)
    }

    fn provider_request(
        &self,
        prompt: &AcceptedKnowledgePrompt,
        packet: &KnowledgeJudgePacket,
    ) -> ProviderCallRequest {
        ProviderCallRequest::new(
            self.configuration.provider_name().clone(),
            self.configuration.model_name().clone(),
            self.configuration.authorization().clone(),
            vec![
                ProviderMessage::system(Self::system_prompt(prompt)),
                ProviderMessage::user(Self::user_prompt(packet)),
            ],
        )
    }

    fn system_prompt(prompt: &AcceptedKnowledgePrompt) -> String {
        format!(
            "{}\n\nReturn exactly one KnowledgeJudgeResponse NOTA value and nothing else. The encoded response is positional; do not prefix it with KnowledgeJudgeResponse. Valid examples include `(Accept None)` and `((Reject NotKnowledge) None)`. Do not return JSON, markdown, or prose around the response.",
            prompt.system_text().trim()
        )
    }

    fn user_prompt(packet: &KnowledgeJudgePacket) -> String {
        format!(
            "KnowledgeJudgePacket under judgment:\n{}\n\nReturn one KnowledgeJudgeResponse.",
            packet.to_nota()
        )
    }

    fn reply_from_provider(&self, reply: ProviderCallReply) -> MindJudgeReply {
        match NotaSource::new(reply.output_text()).parse::<KnowledgeJudgeResponse>() {
            Ok(response) => MindJudgeReply::KnowledgeJudged(response),
            Err(error) => MindJudgeReply::RequestRejected(MindJudgeRequestRejection::new(
                MindJudgeRequestRejectionReason::ResponseFormatFailure,
                Self::message(format!("KnowledgeJudgeResponse: {error}")),
            )),
        }
    }

    fn message(message: String) -> TextBody {
        TextBody::new(message)
            .unwrap_or_else(|_| TextBody::new("mind judge request failed").unwrap())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MindJudgeSocketPath(PathBuf);

impl Error {
    fn from_frame_codec(error: MindJudgeFrameCodecError) -> Self {
        match error {
            MindJudgeFrameCodecError::Io(error) => Self::Socket(error),
            other => Self::Frame(other.to_string()),
        }
    }
}

impl MindJudgeSocketPath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    fn remove_stale(&self) -> Result<(), Error> {
        match std::fs::remove_file(self.as_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::Socket(error)),
        }
    }
}

struct MindJudgeSocketExchange<'a> {
    stream: &'a mut UnixStream,
    codec: MindJudgeFrameCodec,
}

impl<'a> MindJudgeSocketExchange<'a> {
    fn new(stream: &'a mut UnixStream, codec: MindJudgeFrameCodec) -> Self {
        Self { stream, codec }
    }

    async fn read_frame(&mut self) -> Result<MindJudgeFrame, Error> {
        let mut length_prefix = [0_u8; 4];
        self.stream
            .read_exact(&mut length_prefix)
            .await
            .map_err(Error::Socket)?;
        let length = self
            .codec
            .frame_payload_length(length_prefix)
            .map_err(Error::from_frame_codec)?;
        let mut payload = vec![0_u8; length];
        self.stream
            .read_exact(&mut payload)
            .await
            .map_err(Error::Socket)?;
        self.codec
            .decode_frame_bytes(length_prefix, payload)
            .map_err(Error::from_frame_codec)
    }

    async fn write_frame(&mut self, frame: &MindJudgeFrame) -> Result<(), Error> {
        let bytes = self
            .codec
            .encode_frame(frame)
            .map_err(Error::from_frame_codec)?;
        self.stream
            .write_all(bytes.as_slice())
            .await
            .map_err(Error::Socket)?;
        self.stream.flush().await.map_err(Error::Socket)?;
        Ok(())
    }
}

pub struct MindJudgeSocketServer<Client> {
    socket_path: MindJudgeSocketPath,
    adapter: MindJudgeAdapter<Client>,
    codec: MindJudgeFrameCodec,
}

impl<Client> MindJudgeSocketServer<Client>
where
    Client: ProviderClient,
{
    pub fn new(socket_path: MindJudgeSocketPath, adapter: MindJudgeAdapter<Client>) -> Self {
        Self {
            socket_path,
            adapter,
            codec: MindJudgeFrameCodec::default(),
        }
    }

    pub async fn serve_one(self) -> Result<MindJudgeReply, Error> {
        self.socket_path.remove_stale()?;
        let listener = UnixListener::bind(self.socket_path.as_path()).map_err(Error::Socket)?;
        let result = async {
            let (mut stream, _) = listener.accept().await.map_err(Error::Socket)?;
            self.serve_stream(&mut stream).await
        }
        .await;
        self.socket_path.remove_stale()?;
        result
    }

    /// Serve sequential direct-bind connections until no client arrives within
    /// `idle_timeout`. Inherited socket-activation file descriptors are still a
    /// follow-up; this loop is the non-activated semi-persistent runtime path.
    pub async fn serve_until_idle(self, idle_timeout: Duration) -> Result<usize, Error> {
        self.socket_path.remove_stale()?;
        let listener = UnixListener::bind(self.socket_path.as_path()).map_err(Error::Socket)?;
        let mut served = 0;
        let result = async {
            loop {
                let accepted = timeout(idle_timeout, listener.accept()).await;
                let (mut stream, _) = match accepted {
                    Ok(Ok(accepted)) => accepted,
                    Ok(Err(error)) => return Err(Error::Socket(error)),
                    Err(_elapsed) => return Ok(served),
                };
                self.serve_stream(&mut stream).await?;
                served += 1;
            }
        }
        .await;
        self.socket_path.remove_stale()?;
        result
    }

    pub async fn serve_stream(&self, stream: &mut UnixStream) -> Result<MindJudgeReply, Error> {
        let mut exchange = MindJudgeSocketExchange::new(stream, self.codec);
        let frame = exchange.read_frame().await?;
        let received = self
            .codec
            .request_from_frame(frame)
            .map_err(Error::from_frame_codec)?;
        let reply = self.adapter.judge(received.request().clone());
        let frame = received.reply_frame(reply.clone());
        exchange.write_frame(&frame).await?;
        Ok(reply)
    }
}
pub struct MindJudgeClient {
    socket_path: MindJudgeSocketPath,
    codec: MindJudgeFrameCodec,
}

impl MindJudgeClient {
    pub fn new(socket_path: MindJudgeSocketPath) -> Self {
        Self {
            socket_path,
            codec: MindJudgeFrameCodec::default(),
        }
    }

    pub async fn submit(&self, request: MindJudgeRequest) -> Result<MindJudgeReply, Error> {
        let mut stream = UnixStream::connect(self.socket_path.as_path())
            .await
            .map_err(Error::Socket)?;
        let mut exchange = MindJudgeSocketExchange::new(&mut stream, self.codec);
        let frame = self.codec.request_frame(request);
        exchange.write_frame(&frame).await?;
        let frame = exchange.read_frame().await?;
        self.codec
            .reply_from_frame(frame)
            .map_err(Error::from_frame_codec)
    }
}
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use judge::FixtureProviderClient;
    use signal_domain::{Domain, Software, Technology};
    use signal_mind_judge::{KnowledgeJudgeVerdict, TextBody};
    use tempfile::TempDir;

    use super::*;

    struct TestConfiguration {
        temporary: TempDir,
        root: MindJudgeConfigurationPath,
    }

    impl TestConfiguration {
        fn new(prompt_text: &str) -> Self {
            let temporary = TempDir::new().unwrap();
            let prompt_directory = temporary.path().join("prompts/accepted-knowledge");
            std::fs::create_dir_all(&prompt_directory).unwrap();
            std::fs::write(prompt_directory.join("system.md"), prompt_text).unwrap();
            let root = MindJudgeConfigurationPath::new(temporary.path()).unwrap();
            Self { temporary, root }
        }

        fn root(&self) -> MindJudgeConfigurationPath {
            let _keep = self.temporary.path();
            self.root.clone()
        }
    }

    #[test]
    fn fixture_provider_accepts_through_adapter() {
        let configuration = TestConfiguration::new("Prompt from temp config.");
        let adapter = MindJudgeAdapter::new(
            MindJudgeAdapterConfiguration::fixture(configuration.root()),
            FixtureProviderClient::from_text("(Accept None)"),
        );

        let reply = adapter.judge(MindJudgeRequest::JudgeKnowledge(SelfPacket::packet()));

        assert_eq!(
            reply,
            MindJudgeReply::KnowledgeJudged(KnowledgeJudgeResponse::new(
                KnowledgeJudgeVerdict::Accept,
                None,
            ))
        );
    }

    #[test]
    fn malformed_provider_text_maps_to_request_rejection() {
        let configuration = TestConfiguration::new("Prompt from temp config.");
        let adapter = MindJudgeAdapter::new(
            MindJudgeAdapterConfiguration::fixture(configuration.root()),
            FixtureProviderClient::from_text("not nota"),
        );

        let reply = adapter.judge(MindJudgeRequest::JudgeKnowledge(SelfPacket::packet()));

        assert!(matches!(
            reply,
            MindJudgeReply::RequestRejected(MindJudgeRequestRejection {
                reason: MindJudgeRequestRejectionReason::ResponseFormatFailure,
                ..
            })
        ));
    }

    #[test]
    fn prompt_is_read_from_runtime_config_root() {
        let configuration = TestConfiguration::new("Runtime prompt marker.");

        let prompt = AcceptedKnowledgePrompt::read_from(&configuration.root()).unwrap();

        assert_eq!(prompt.system_text(), "Runtime prompt marker.");
    }

    #[tokio::test]
    async fn unix_socket_frame_exchange_succeeds() {
        let configuration = TestConfiguration::new("Prompt from temp config.");
        let socket = configuration.temporary.path().join("mind-judge.sock");
        let server = MindJudgeSocketServer::new(
            MindJudgeSocketPath::new(&socket),
            MindJudgeAdapter::new(
                MindJudgeAdapterConfiguration::fixture(configuration.root()),
                FixtureProviderClient::from_text("(Accept None)"),
            ),
        );
        let server_task = tokio::spawn(server.serve_one());
        tokio::time::sleep(Duration::from_millis(25)).await;
        let client = MindJudgeClient::new(MindJudgeSocketPath::new(socket));

        let reply = client
            .submit(MindJudgeRequest::JudgeKnowledge(SelfPacket::packet()))
            .await
            .unwrap();
        let served_reply = server_task.await.unwrap().unwrap();

        assert_eq!(reply, served_reply);
        assert!(matches!(reply, MindJudgeReply::KnowledgeJudged(_)));
    }

    #[tokio::test]
    async fn direct_bind_server_handles_multiple_sequential_requests() {
        let configuration = TestConfiguration::new("Prompt from temp config.");
        let socket = configuration.temporary.path().join("mind-judge-loop.sock");
        let server = MindJudgeSocketServer::new(
            MindJudgeSocketPath::new(&socket),
            MindJudgeAdapter::new(
                MindJudgeAdapterConfiguration::fixture(configuration.root()),
                FixtureProviderClient::from_text("(Accept None)"),
            ),
        );
        let server_task = tokio::spawn(server.serve_until_idle(Duration::from_millis(50)));
        tokio::time::sleep(Duration::from_millis(25)).await;
        let client = MindJudgeClient::new(MindJudgeSocketPath::new(socket));

        for _ in 0..2 {
            let reply = client
                .submit(MindJudgeRequest::JudgeKnowledge(SelfPacket::packet()))
                .await
                .unwrap();
            assert!(matches!(reply, MindJudgeReply::KnowledgeJudged(_)));
        }

        assert_eq!(server_task.await.unwrap().unwrap(), 2);
    }

    struct SelfPacket;

    impl SelfPacket {
        fn packet() -> KnowledgeJudgePacket {
            KnowledgeJudgePacket::new(
                Domain::Technology(Technology::Software(Software::Engineering(
                    signal_domain::EngineeringLeaf::Architecture,
                ))),
                TextBody::new("Mind judge tests use typed frames.").unwrap(),
                Vec::new(),
            )
        }
    }
}
