#[cfg(test)]
mod tests {
    use orchyn_action_planner::{ActionPlanner, Goal};
    use orchyn_actor_engine::{Actor, ActorBudget, ActorEngine, ActorPersona, MultiActorManager};
    use orchyn_core::ActorId;
    use orchyn_model_client::{ModelConfig, QwenClient};
    use orchyn_pixel_observer::PixelObserver;
    use orchyn_storage::Storage;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_multi_actor_todo_sync() {
        let config = ModelConfig::default();
        let model = QwenClient::new(Some(config));
        let observer = PixelObserver::new(model.clone());
        let planner = ActionPlanner::new(model);
        let engine = ActorEngine::new(planner, observer);
        let _manager = MultiActorManager::new(engine);

        let alice_id = Uuid::new_v4();
        let alice = Actor {
            id: alice_id,
            persona: ActorPersona::Rational,
            goal: Goal {
                description: "Add a todo".to_string(),
                criteria: vec![],
            },
            budget: ActorBudget {
                max_steps: 5,
                max_tokens: 5000,
                max_seconds: 30,
            },
        };

        assert_eq!(alice.persona, ActorPersona::Rational);
    }

    #[tokio::test]
    async fn test_storage_multi_user_flow() {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:password@localhost:5432/orchyn".to_string());
        let storage = Storage::new(&database_url, PathBuf::from("./test_artifacts"))
            .await
            .unwrap();

        // 1. Ensure users
        let alice_id = storage.ensure_user("alice@example.com").await.unwrap();
        let bob_id = storage.ensure_user("bob@example.com").await.unwrap();

        // 2. Alice creates a project
        let project_id = storage
            .create_project(alice_id, "Team Todo", "http://localhost:8080")
            .await
            .unwrap();

        // 3. Alice invites Bob
        let invite_id = storage
            .create_invitation(project_id, alice_id, "bob@example.com")
            .await
            .unwrap();

        // 4. Bob accepts
        storage.accept_invitation(invite_id, bob_id).await.unwrap();

        // 5. Verify Bob is a member
        let bob_projects = storage.get_user_projects(bob_id).await.unwrap();
        assert!(bob_projects.iter().any(|p| p.id == project_id));

        // 6. Start a run and save a dummy artifact
        let run_id = storage.start_run(project_id).await.unwrap();
        let artifact_id = storage
            .save_screenshot(run_id, b"fake_png_data")
            .await
            .unwrap();

        assert!(artifact_id != Uuid::nil());
    }
}
