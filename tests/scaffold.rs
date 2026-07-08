use mind_judge::{MindJudgeAdapter, MindJudgeAdapterConfiguration, MindJudgeConfigurationPath};

#[test]
fn adapter_configuration_names_external_prompt_root() {
    let configuration = MindJudgeAdapterConfiguration::new(
        MindJudgeConfigurationPath::new("/tmp/mind-judge-config").unwrap(),
        judge::ProviderName::new("provider").unwrap(),
        judge::ProviderModelName::new("model").unwrap(),
        judge::ProviderAuthorization::secret_source(
            judge::SecretSourceReference::new("secret-handle").unwrap(),
        ),
    );
    let adapter = MindJudgeAdapter::new(
        configuration,
        judge::FixtureProviderClient::from_text("(Accept None)"),
    );

    assert_eq!(
        adapter.configuration().configuration_path().as_path(),
        std::path::Path::new("/tmp/mind-judge-config")
    );
}
