use orchyn_harness::{OrchynHarness, HarnessConfig, MockLlm, Phase, Verdict};

#[tokio::test]
async fn test_harness_phase_transitions() {
    let config = HarnessConfig {
        target_url: "http://localhost:8080".to_string(),
        max_fix_iterations: 3,
    };
    let llm = MockLlm { should_succeed: true };
    let mut harness = OrchynHarness::new(config, llm);

    // Test Ingest phase
    let evidence = harness.run_phase(Phase::Ingest).await.unwrap();
    assert_eq!(evidence.phase, Phase::Ingest);
    assert_eq!(evidence.verdict, Verdict::Pass);

    // Test FixLoop phase (Mock failure)
    let evidence = harness.run_phase(Phase::FixLoop).await.unwrap();
    assert_eq!(evidence.phase, Phase::FixLoop);
    assert_eq!(evidence.verdict, Verdict::Pass); // MockLLM always succeeds for now
}

#[tokio::test]
async fn test_harness_full_flow() {
    let config = HarnessConfig {
        target_url: "http://localhost:8080".to_string(),
        max_fix_iterations: 3,
    };
    let llm = MockLlm { should_succeed: true };
    let mut harness = OrchynHarness::new(config, llm);

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
    };
    let llm = MockLlm { should_succeed: true };
    let mut harness = OrchynHarness::new(config, llm);

    let evidences = harness.run_all().await.unwrap();
    assert!(evidences.len() >= 5); // Ingest, Vision, Single, Multi, Ship
    assert_eq!(evidences.last().unwrap().phase, Phase::Ship);
}
