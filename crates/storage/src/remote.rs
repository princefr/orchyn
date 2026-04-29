use crate::types::{ProjectRow, RunRow};
use crate::traits::StorageProviderInner;
use orchyn_core::{OrchynError, Result};
use std::sync::Arc;
use uuid::Uuid;
use reqwest::Client;
use std::future::Future;

pub struct RemoteStorage {
    server_url: String,
    api_key: String,
    client: Client,
    artifact_dir: std::path::PathBuf,
}

impl RemoteStorage {
    pub fn new(server_url: String, api_key: String) -> Self {
        Self {
            server_url,
            api_key,
            client: Client::new(),
            artifact_dir: std::path::PathBuf::from("./remote_artifacts"),
        }
    }
}

impl StorageProviderInner for RemoteStorage {
    fn ensure_user(&self, email: &str) -> impl Future<Output = std::result::Result<Uuid, OrchynError>> + Send {
        let url = format!("{}/api/v1/users/ensure", self.server_url);
        let client = self.client.clone();
        let email = email.to_string();
        async move {
            let res = client.post(url).json(&serde_json::json!({"email": email})).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn create_project(&self, owner_id: Uuid, name: &str, source_url: &str) -> impl Future<Output = std::result::Result<Uuid, OrchynError>> + Send {
        let url = format!("{}/api/v1/projects", self.server_url);
        let client = self.client.clone();
        let name = name.to_string();
        let source_url = source_url.to_string();
        async move {
            let res = client.post(url).json(&serde_json::json!({"owner_id": owner_id, "name": name, "source_url": source_url})).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn add_project_member(&self, project_id: Uuid, user_id: Uuid, role: &str) -> impl Future<Output = std::result::Result<(), OrchynError>> + Send {
        let url = format!("{}/api/v1/projects/{}/members", self.server_url, project_id);
        let client = self.client.clone();
        let role = role.to_string();
        async move {
            client.post(url).json(&serde_json::json!({"user_id": user_id, "role": role})).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        }
    }

    fn get_user_projects(&self, user_id: Uuid) -> impl Future<Output = std::result::Result<Vec<ProjectRow>, OrchynError>> + Send {
        let url = format!("{}/api/v1/users/{}/projects", self.server_url, user_id);
        let client = self.client.clone();
        async move {
            let res = client.get(url).send().await.map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn create_invitation(&self, project_id: Uuid, inviter_id: Uuid, invitee_email: &str) -> impl Future<Output = std::result::Result<Uuid, OrchynError>> + Send {
        let url = format!("{}/api/v1/projects/{}/invitations", self.server_url, project_id);
        let client = self.client.clone();
        let invitee_email = invitee_email.to_string();
        async move {
            let res = client.post(url).json(&serde_json::json!({"inviter_id": inviter_id, "invitee_email": invitee_email})).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn accept_invitation(&self, invitation_id: Uuid, user_id: Uuid) -> impl Future<Output = std::result::Result<(), OrchynError>> + Send {
        let url = format!("{}/api/v1/invitations/{}/accept", self.server_url, invitation_id);
        let client = self.client.clone();
        async move {
            client.post(url).json(&serde_json::json!({"user_id": user_id})).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        }
    }

    fn start_run(&self, project_id: Uuid) -> impl Future<Output = std::result::Result<Uuid, OrchynError>> + Send {
        let url = format!("{}/api/v1/projects/{}/runs", self.server_url, project_id);
        let client = self.client.clone();
        async move {
            let res = client.post(url).send().await.map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn update_run_status(&self, run_id: Uuid, status: &str) -> impl Future<Output = std::result::Result<(), OrchynError>> + Send {
        let url = format!("{}/api/v1/runs/{}", self.server_url, run_id);
        let client = self.client.clone();
        let status = status.to_string();
        async move {
            client.patch(url).json(&serde_json::json!({"status": status})).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        }
    }

    fn get_project_runs(&self, project_id: Uuid) -> impl Future<Output = std::result::Result<Vec<RunRow>, OrchynError>> + Send {
        let url = format!("{}/api/v1/projects/{}/runs", self.server_url, project_id);
        let client = self.client.clone();
        async move {
            let res = client.get(url).send().await.map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn get_run(&self, run_id: Uuid) -> impl Future<Output = std::result::Result<RunRow, OrchynError>> + Send {
        let url = format!("{}/api/v1/runs/{}", self.server_url, run_id);
        let client = self.client.clone();
        async move {
            let res = client.get(url).send().await.map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn update_run_diagnostics(&self, run_id: Uuid, events: Vec<orchyn_core::DiagnosticEvent>) -> impl Future<Output = std::result::Result<(), OrchynError>> + Send {
        let url = format!("{}/api/v1/runs/{}/diagnostics", self.server_url, run_id);
        let client = self.client.clone();
        async move {
            client.post(url).json(&events).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        }
    }

    fn save_screenshot(&self, run_id: Uuid, data: &[u8]) -> impl Future<Output = std::result::Result<Uuid, OrchynError>> + Send {
        let url = format!("{}/api/v1/runs/{}/screenshots", self.server_url, run_id);
        let client = self.client.clone();
        let data = data.to_vec();
        async move {
            let res = client.post(url).body(data).send().await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            res.json().await.map_err(|e| OrchynError::Storage(e.to_string()))
        }
    }

    fn artifact_dir(&self) -> &std::path::Path {
        &self.artifact_dir
    }
}
