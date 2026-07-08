use mind_judge::{MindJudgeAdapter, MindJudgeAdapterConfiguration, MindJudgeConfigurationPath};

#[test]
fn adapter_configuration_names_external_prompt_root() {
    let configuration = MindJudgeAdapterConfiguration::new(
        MindJudgeConfigurationPath::new("/tmp/mind-judge-config").unwrap(),
        judge::ProviderName::new("provider"),
        judge::ProviderModelName::new("model"),
        judge::SecretSourceReference::new("secret-handle"),
    );
    let adapter = MindJudgeAdapter::new(configuration);

    assert_eq!(
        adapter.configuration().configuration_path().as_path(),
        std::path::Path::new("/tmp/mind-judge-config")
    );
}
