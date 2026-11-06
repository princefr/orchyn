use orchyn_harness::{HarnessConfig, MockLlm, OrchynHarness, Phase, Verdict};
use orchyn_model_client::MockClient;
use orchyn_storage::Storage;
use orchyn_artifact_storage::MockArtifactStorage;
use std::sync::Arc;

#[tokio::test]
async fn test_harness_phase_transitions() {
    let config = HarnessConfig {
        target_url: "http://localhost:8080".to_string(),
        max_fix_iterations: 3,
        phases: None,
    };
    let model = Arc::new(MockClient::new(vec![]));
    let llm = MockLlm { client: model };
    
    let mock_db = Arc::new(sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap());
    let storage = Arc::new(Storage::new(mock_db));
    let artifact_storage = Arc::new(MockArtifactStorage::new());

    let harness = OrchynHarness::new(storage, artifact_storage, config, llm);

    // Test Ingest phase
    let evidence = harness.run_phase(Phase::Ingest).await.unwrap();
    assert_eq!(evidence.phase, Phase::Ingest);
    assert_eq!(evidence.verdict, Verdict::Pass);

    // Test FixLoop phase
    let evidence = harness.run_phase(Phase::FixLoop).await.unwrap();
    assert_eq!(evidence.phase, Phase::FixLoop);
    assert_eq!(evidence.verdict, Verdict::Pass);
}

#[tokio::test]
async fn test_harness_full_flow() {
    let config = HarnessConfig {
        target_url: "http://localhost:8080".to_string(),
        max_fix_iterations: 3,
        phases: None,
    };
    let model = Arc::new(MockClient::new(vec![]));
    let llm = MockLlm { client: model };
    
    let mock_db = Arc::new(sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap());
    let storage = Arc::new(Storage::new(mock_db));
    let artifact_storage = Arc::new(MockArtifactStorage::new());

    let harness = OrchynHarness::new(storage, artifact_storage, config, llm);

    let phases = vec![
        Phase::Ingest,
        Phase::VisionBaseline,
        Phase::SingleActorFlow,
        Phase::MultiActorFlow,
        Phase::ErrorConstraint,
        Phase::FixLoop,
        Phase::Harden,
        Phase::Ship,
    ];

    for phase in phases {
        let evidence = harness.run_phase(phase.clone()).await.unwrap();
        assert_eq!(evidence.phase, phase);
        assert_eq!(evidence.verdict, Verdict::Pass);
    }
}

#[tokio::test]
async fn test_harness_run_all() {
    let config = HarnessConfig {
        target_url: "http://localhost:8080".to_string(),
        max_fix_iterations: 3,
        phases: None,
    };
    let model = Arc::new(MockClient::new(vec![]));
    let llm = MockLlm { client: model };
    
    let mock_db = Arc::new(sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap());
    let storage = Arc::new(Storage::new(mock_db));
    let artifact_storage = Arc::new(MockArtifactStorage::new());

    let harness = OrchynHarness::new(storage, artifact_storage, config, llm);

    let results = harness.run_all().await.unwrap();
    assert!(results.len() >= 5);
    assert_eq!(results.last().unwrap().phase, Phase::Ship);
}
