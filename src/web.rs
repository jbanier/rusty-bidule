use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{Mutex, mpsc::unbounded_channel};
use tracing::info;

use crate::{
    config::AppConfig,
    orchestrator::Orchestrator,
    paths::discover_project_root,
    prompt_expansion::expand_prompt_file_references,
    recipes::RecipeRegistry,
    types::{ConversationSummary, ProgressEvent, RunTurnResult, UiEvent},
};

static INDEX_HTML: &str = include_str!("static/index.html");

// ---------------------------------------------------------------------------
// Job registry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct JobState {
    pub conversation_id: Option<String>,
    pub status: String,
    pub events: Vec<ProgressEvent>,
    pub result: Option<RunTurnResult>,
    pub error: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub type JobRegistry = Arc<Mutex<HashMap<String, JobState>>>;

const COMPLETED_JOB_TTL: Duration = Duration::from_secs(60 * 60);
const JOB_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

fn new_job_id() -> String {
    let suffix = rand::random::<u32>();
    format!("job-{suffix:08x}")
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct WebAppState {
    pub orchestrator: Orchestrator,
    pub jobs: JobRegistry,
    pub recipes: Arc<RecipeRegistry>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ConvoSummaryResponse {
    conversation_id: String,
    updated_at: String,
    message_count: usize,
}

impl From<ConversationSummary> for ConvoSummaryResponse {
    fn from(s: ConversationSummary) -> Self {
        Self {
            conversation_id: s.conversation_id,
            updated_at: s.updated_at.to_rfc3339(),
            message_count: s.message_count,
        }
    }
}

#[derive(Deserialize)]
struct PostMessageBody {
    content: String,
}

#[derive(Deserialize)]
struct RecipeUseBody {
    name: String,
}

#[derive(Deserialize)]
struct McpServersBody {
    enabled: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ConfigUpdateBody {
    config: AppConfig,
}

#[derive(Serialize)]
struct ScratchpadResponse {
    body: String,
}

#[derive(Deserialize)]
struct ScratchpadBody {
    body: String,
}

#[derive(Deserialize)]
struct FindingCreateBody {
    kind: String,
    value: String,
    note: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

// ---------------------------------------------------------------------------
// Error helper
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = json!({"error": self.0.to_string()});
        let status = if body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not found")
        {
            StatusCode::NOT_FOUND
        } else if body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid")
        {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, axum::Json(body)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}

type AppResult<T> = std::result::Result<T, AppError>;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(json!({"status": "ok"}))
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn list_conversations(
    State(state): State<WebAppState>,
) -> AppResult<axum::Json<serde_json::Value>> {
    let summaries = state.orchestrator.store().list_conversations()?;
    let resp: Vec<ConvoSummaryResponse> = summaries.into_iter().map(Into::into).collect();
    Ok(axum::Json(json!(resp)))
}

async fn create_conversation(
    State(state): State<WebAppState>,
) -> AppResult<axum::Json<serde_json::Value>> {
    let convo = state.orchestrator.store().create_conversation()?;
    Ok(axum::Json(serde_json::to_value(&convo)?))
}

async fn get_conversation(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let convo = state.orchestrator.store().load(&id)?;
    Ok(axum::Json(serde_json::to_value(&convo)?))
}

async fn delete_conversation(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    validate_resource_id(&id, "conversation")?;
    state.orchestrator.store().delete(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn post_message(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<PostMessageBody>,
) -> AppResult<(StatusCode, axum::Json<serde_json::Value>)> {
    validate_resource_id(&id, "conversation")?;
    let conversation = state.orchestrator.store().load(&id)?;
    let content = expand_prompt_file_references(
        &body.content,
        &conversation.agent_permissions,
        discover_project_root().as_deref(),
    )?;
    ensure_no_active_job_for_conversation(&state, &id).await?;
    let job_id = new_job_id();
    {
        let mut jobs = state.jobs.lock().await;
        jobs.insert(
            job_id.clone(),
            JobState {
                conversation_id: Some(id.clone()),
                status: "running".to_string(),
                events: Vec::new(),
                result: None,
                error: None,
                completed_at: None,
            },
        );
    }

    let orchestrator = state.orchestrator.clone();
    let jobs = state.jobs.clone();
    let conversation_id = id.clone();
    let job_id_clone = job_id.clone();
    tokio::spawn(async move {
        let (ui_tx, mut ui_rx) = unbounded_channel::<UiEvent>();

        // Collect events in parallel while running
        let jobs_ev = jobs.clone();
        let job_id_ev = job_id_clone.clone();
        tokio::spawn(async move {
            while let Some(event) = ui_rx.recv().await {
                match event {
                    UiEvent::Progress(p) => {
                        let mut lock = jobs_ev.lock().await;
                        if let Some(job) = lock.get_mut(&job_id_ev) {
                            job.events.push(p);
                        }
                    }
                    UiEvent::Finished(_) | UiEvent::CompactionFinished(_) => {}
                }
            }
        });

        let result = orchestrator
            .run_turn(&conversation_id, content, ui_tx)
            .await;

        let mut lock = jobs.lock().await;
        if let Some(job) = lock.get_mut(&job_id_clone) {
            match result {
                Ok(r) => {
                    mark_job_completed(job, "done", Some(r), None);
                }
                Err(e) => {
                    mark_job_completed(job, "failed", None, Some(format!("{e:#}")));
                }
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        axum::Json(json!({
            "job_id": job_id,
            "conversation_id": id,
        })),
    ))
}

async fn compact_conversation(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, axum::Json<serde_json::Value>)> {
    validate_resource_id(&id, "conversation")?;
    ensure_no_active_job_for_conversation(&state, &id).await?;
    let job_id = new_job_id();
    {
        let mut jobs = state.jobs.lock().await;
        jobs.insert(
            job_id.clone(),
            JobState {
                conversation_id: Some(id.clone()),
                status: "running".to_string(),
                events: Vec::new(),
                result: None,
                error: None,
                completed_at: None,
            },
        );
    }

    let orchestrator = state.orchestrator.clone();
    let jobs = state.jobs.clone();
    let conversation_id = id.clone();
    let job_id_clone = job_id.clone();
    tokio::spawn(async move {
        let (ui_tx, _ui_rx) = unbounded_channel::<UiEvent>();
        let result = orchestrator
            .compact_conversation(&conversation_id, ui_tx)
            .await;
        let mut lock = jobs.lock().await;
        if let Some(job) = lock.get_mut(&job_id_clone) {
            match result {
                Ok(summary) => mark_job_completed(
                    job,
                    "done",
                    Some(RunTurnResult {
                        reply: summary,
                        tool_calls: 0,
                    }),
                    None,
                ),
                Err(err) => {
                    mark_job_completed(job, "failed", None, Some(format!("{err:#}")));
                }
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        axum::Json(json!({"job_id": job_id, "conversation_id": id})),
    ))
}

async fn list_recipes(
    State(state): State<WebAppState>,
) -> AppResult<axum::Json<serde_json::Value>> {
    let recipes = state
        .recipes
        .list()
        .iter()
        .map(|r| {
            json!({
                "name": r.name,
                "title": r.title,
                "description": r.description,
                "keywords": r.keywords,
            })
        })
        .collect::<Vec<_>>();
    Ok(axum::Json(json!(recipes)))
}

async fn use_recipe(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<RecipeUseBody>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let store = state.orchestrator.store();
    let mut convo = store.load(&id)?;
    convo.pending_recipe = Some(body.name);
    store.save(&convo)?;
    Ok(axum::Json(serde_json::to_value(&convo)?))
}

async fn clear_recipe(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let store = state.orchestrator.store();
    let mut convo = store.load(&id)?;
    convo.pending_recipe = None;
    store.save(&convo)?;
    Ok(axum::Json(serde_json::to_value(&convo)?))
}

async fn get_mcp_servers(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let convo = state.orchestrator.store().load(&id)?;
    Ok(axum::Json(json!({"enabled": convo.enabled_mcp_servers})))
}

async fn put_mcp_servers(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<McpServersBody>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let store = state.orchestrator.store();
    let mut convo = store.load(&id)?;
    convo.enabled_mcp_servers = body.enabled;
    store.save(&convo)?;
    Ok(axum::Json(json!({"enabled": convo.enabled_mcp_servers})))
}

async fn delete_mcp_servers(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    validate_resource_id(&id, "conversation")?;
    let store = state.orchestrator.store();
    let mut convo = store.load(&id)?;
    convo.enabled_mcp_servers = None;
    store.save(&convo)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_conversation_jobs(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let jobs = state.orchestrator.store().load_job_state(&id)?;
    Ok(axum::Json(json!(jobs)))
}

async fn get_scratchpad(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let body = state.orchestrator.store().load_scratchpad(&id)?;
    Ok(axum::Json(serde_json::to_value(ScratchpadResponse {
        body,
    })?))
}

async fn put_scratchpad(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<ScratchpadBody>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    state
        .orchestrator
        .store()
        .save_scratchpad(&id, &body.body)?;
    Ok(axum::Json(serde_json::to_value(ScratchpadResponse {
        body: body.body,
    })?))
}

async fn list_findings(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&id, "conversation")?;
    let findings = state
        .orchestrator
        .store()
        .load_findings()?
        .into_iter()
        .filter(|finding| finding.conversation_id == id)
        .collect::<Vec<_>>();
    Ok(axum::Json(serde_json::to_value(findings)?))
}

async fn create_finding(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<FindingCreateBody>,
) -> AppResult<(StatusCode, axum::Json<serde_json::Value>)> {
    validate_resource_id(&id, "conversation")?;
    let finding = state.orchestrator.store().add_finding(
        &id,
        &body.kind,
        &body.value,
        body.note.as_deref(),
    )?;
    Ok((
        StatusCode::CREATED,
        axum::Json(serde_json::to_value(finding)?),
    ))
}

async fn delete_finding(
    State(state): State<WebAppState>,
    Path(finding_id): Path<String>,
) -> AppResult<StatusCode> {
    validate_resource_id(&finding_id, "finding")?;
    if state.orchestrator.store().remove_finding(&finding_id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(anyhow::anyhow!("finding '{finding_id}' not found").into())
    }
}

async fn search_local(
    State(state): State<WebAppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<axum::Json<serde_json::Value>> {
    let needle = query.q.trim();
    if needle.is_empty() {
        return Err(anyhow::anyhow!("invalid search query").into());
    }
    let results = state.orchestrator.store().search_local(needle)?;
    Ok(axum::Json(serde_json::to_value(results)?))
}

async fn get_config(State(state): State<WebAppState>) -> AppResult<axum::Json<serde_json::Value>> {
    Ok(axum::Json(serde_json::to_value(
        state.orchestrator.config(),
    )?))
}

async fn put_config(
    axum::Json(body): axum::Json<ConfigUpdateBody>,
) -> AppResult<axum::Json<serde_json::Value>> {
    let path = current_config_path();
    body.config.save(&path)?;
    Ok(axum::Json(json!({
        "saved_to": path,
        "reload_required": true
    })))
}

async fn list_oauth_servers(
    State(state): State<WebAppState>,
) -> AppResult<axum::Json<serde_json::Value>> {
    let servers = state
        .orchestrator
        .config()
        .mcp_servers
        .into_iter()
        .filter(|server| {
            matches!(
                server.auth,
                Some(crate::config::McpAuthConfig::OauthPublic(_))
            )
        })
        .map(|server| json!({"name": server.name, "url": server.url}))
        .collect::<Vec<_>>();
    Ok(axum::Json(json!(servers)))
}

async fn start_oauth_server(
    State(state): State<WebAppState>,
    Path(server_name): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&server_name, "server")?;
    state.orchestrator.login_mcp_server(&server_name).await?;
    Ok(axum::Json(
        json!({"server": server_name, "status": "authorized"}),
    ))
}

async fn oauth_callback(Path(server_name): Path<String>) -> Html<String> {
    Html(format!(
        "<html><body><h1>OAuth callback received for {}</h1><p>The Rust runtime currently completes OAuth through the MCP login flow.</p></body></html>",
        server_name
    ))
}

async fn get_job(
    State(state): State<WebAppState>,
    Path(job_id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    validate_resource_id(&job_id, "job")?;
    let jobs = state.jobs.lock().await;
    let job = jobs
        .get(&job_id)
        .ok_or_else(|| anyhow::anyhow!("job '{job_id}' not found"))?;
    Ok(axum::Json(serde_json::to_value(job)?))
}

async fn delete_job(
    State(state): State<WebAppState>,
    Path(job_id): Path<String>,
) -> AppResult<StatusCode> {
    validate_resource_id(&job_id, "job")?;
    let mut jobs = state.jobs.lock().await;
    jobs.remove(&job_id)
        .ok_or_else(|| anyhow::anyhow!("job '{job_id}' not found"))?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Router & server entry point
// ---------------------------------------------------------------------------

pub async fn run_web_server(
    orchestrator: Orchestrator,
    recipes: RecipeRegistry,
    host: &str,
    port: u16,
) -> Result<()> {
    let state = WebAppState {
        orchestrator,
        jobs: Arc::new(Mutex::new(HashMap::new())),
        recipes: Arc::new(recipes),
    };
    spawn_job_registry_sweeper(state.jobs.clone());

    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/oauth/callback/{server_name}", get(oauth_callback))
        .route("/api/config", get(get_config).put(put_config))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/api/conversations/{id}",
            get(get_conversation).delete(delete_conversation),
        )
        .route("/api/conversations/{id}/messages", post(post_message))
        .route(
            "/api/conversations/{id}/compact",
            post(compact_conversation),
        )
        .route(
            "/api/conversations/{id}/recipe",
            post(use_recipe).delete(clear_recipe),
        )
        .route(
            "/api/conversations/{id}/mcp-servers",
            get(get_mcp_servers)
                .put(put_mcp_servers)
                .delete(delete_mcp_servers),
        )
        .route("/api/conversations/{id}/jobs", get(list_conversation_jobs))
        .route(
            "/api/conversations/{id}/scratchpad",
            get(get_scratchpad).put(put_scratchpad),
        )
        .route(
            "/api/conversations/{id}/findings",
            get(list_findings).post(create_finding),
        )
        .route(
            "/api/findings/{finding_id}",
            axum::routing::delete(delete_finding),
        )
        .route("/api/search", get(search_local))
        .route("/api/mcp/oauth-servers", get(list_oauth_servers))
        .route(
            "/api/mcp/oauth-servers/{server_name}/start",
            post(start_oauth_server),
        )
        .route("/api/recipes", get(list_recipes))
        .route("/api/jobs/{job_id}", get(get_job).delete(delete_job))
        .with_state(state);

    let addr = format!("{host}:{port}");
    info!(addr, "starting web server");
    println!("Web interface running at http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ensure_no_active_job_for_conversation(
    state: &WebAppState,
    conversation_id: &str,
) -> AppResult<()> {
    let jobs = state.jobs.lock().await;
    if jobs.values().any(|job| {
        job.status == "running" && job.conversation_id.as_deref() == Some(conversation_id)
    }) {
        return Err(anyhow::anyhow!(
            "conversation '{conversation_id}' already has a running web job"
        )
        .into());
    }
    Ok(())
}

fn validate_resource_id(value: &str, kind: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(anyhow::anyhow!("invalid {kind} id '{value}'").into());
    }
    Ok(())
}

fn current_config_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("RUSTY_BIDULE_CONFIG") {
        return path.into();
    }
    std::path::PathBuf::from("config/config.local.yaml")
}

fn mark_job_completed(
    job: &mut JobState,
    status: &str,
    result: Option<RunTurnResult>,
    error: Option<String>,
) {
    job.status = status.to_string();
    job.result = result;
    job.error = error;
    job.completed_at = Some(Utc::now());
}

fn evict_expired_jobs(jobs: &mut HashMap<String, JobState>, now: DateTime<Utc>, ttl: Duration) {
    let ttl = chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::seconds(0));
    jobs.retain(|_, job| {
        let Some(completed_at) = job.completed_at else {
            return true;
        };
        completed_at + ttl > now
    });
}

fn spawn_job_registry_sweeper(jobs: JobRegistry) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(JOB_SWEEP_INTERVAL);
        loop {
            interval.tick().await;
            let mut lock = jobs.lock().await;
            evict_expired_jobs(&mut lock, Utc::now(), COMPLETED_JOB_TTL);
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        extract::{Path, State},
        response::IntoResponse,
    };
    use chrono::Duration as ChronoDuration;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    use crate::{
        config::{AppConfig, LocalToolsConfig, McpRuntimeConfig},
        orchestrator::Orchestrator,
        recipes::RecipeRegistry,
        types::AgentPermissions,
    };

    use super::{
        COMPLETED_JOB_TTL, FindingCreateBody, JobState, PostMessageBody, ScratchpadBody,
        SearchQuery, WebAppState, create_finding, evict_expired_jobs, get_scratchpad,
        list_findings, post_message, put_scratchpad, search_local,
    };

    fn test_config(data_dir: std::path::PathBuf) -> AppConfig {
        AppConfig {
            prompt: None,
            data_dir: Some(data_dir),
            azure_openai: None,
            agent_permissions: AgentPermissions::default(),
            local_tools: LocalToolsConfig::default(),
            mcp_runtime: McpRuntimeConfig::default(),
            mcp_servers: Vec::new(),
            tracing: None,
        }
    }

    #[test]
    fn evicts_only_completed_jobs_past_ttl() {
        let now = chrono::Utc::now();
        let mut jobs = HashMap::from([
            (
                "running".to_string(),
                JobState {
                    conversation_id: None,
                    status: "running".to_string(),
                    events: Vec::new(),
                    result: None,
                    error: None,
                    completed_at: None,
                },
            ),
            (
                "fresh".to_string(),
                JobState {
                    conversation_id: None,
                    status: "done".to_string(),
                    events: Vec::new(),
                    result: None,
                    error: None,
                    completed_at: Some(now - ChronoDuration::minutes(10)),
                },
            ),
            (
                "expired".to_string(),
                JobState {
                    conversation_id: None,
                    status: "failed".to_string(),
                    events: Vec::new(),
                    result: None,
                    error: Some("boom".to_string()),
                    completed_at: Some(now - ChronoDuration::minutes(61)),
                },
            ),
        ]);

        evict_expired_jobs(&mut jobs, now, COMPLETED_JOB_TTL);

        assert!(jobs.contains_key("running"));
        assert!(jobs.contains_key("fresh"));
        assert!(!jobs.contains_key("expired"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn post_message_rejects_invalid_file_reference_before_creating_job() {
        let dir = tempdir().unwrap();
        let orchestrator = Orchestrator::new(test_config(dir.path().to_path_buf())).unwrap();
        let conversation = orchestrator.store().create_conversation().unwrap();
        let state = WebAppState {
            orchestrator,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            recipes: Arc::new(RecipeRegistry::default()),
        };

        let err = post_message(
            State(state.clone()),
            Path(conversation.conversation_id.clone()),
            axum::Json(PostMessageBody {
                content: "Use @missing.md".to_string(),
            }),
        )
        .await
        .unwrap_err();

        let response = err.into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(state.jobs.lock().await.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scratchpad_round_trip_via_handlers() {
        let dir = tempdir().unwrap();
        let orchestrator = Orchestrator::new(test_config(dir.path().to_path_buf())).unwrap();
        let conversation = orchestrator.store().create_conversation().unwrap();
        let state = WebAppState {
            orchestrator,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            recipes: Arc::new(RecipeRegistry::default()),
        };

        let response = put_scratchpad(
            State(state.clone()),
            Path(conversation.conversation_id.clone()),
            axum::Json(ScratchpadBody {
                body: "working note".to_string(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(response.0["body"], "working note");

        let response = get_scratchpad(State(state), Path(conversation.conversation_id))
            .await
            .unwrap();
        let value = response.0;
        assert_eq!(value["body"], "working note");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn findings_and_search_handlers_return_local_state() {
        let dir = tempdir().unwrap();
        let orchestrator = Orchestrator::new(test_config(dir.path().to_path_buf())).unwrap();
        let conversation = orchestrator.store().create_conversation().unwrap();
        orchestrator
            .store()
            .append_message(&conversation.conversation_id, "user", "Tracking 5.6.7.8")
            .unwrap();
        let state = WebAppState {
            orchestrator,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            recipes: Arc::new(RecipeRegistry::default()),
        };

        let response = create_finding(
            State(state.clone()),
            Path(conversation.conversation_id.clone()),
            axum::Json(FindingCreateBody {
                kind: "ip".to_string(),
                value: "5.6.7.8".to_string(),
                note: Some("watchlist".to_string()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(response.0, axum::http::StatusCode::CREATED);

        let findings = list_findings(State(state.clone()), Path(conversation.conversation_id))
            .await
            .unwrap()
            .0;
        assert_eq!(findings.as_array().unwrap().len(), 1);

        let results = search_local(
            State(state),
            axum::extract::Query(SearchQuery {
                q: "5.6.7.8".to_string(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(!results.as_array().unwrap().is_empty());
    }
}
