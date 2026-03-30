use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{Mutex, mpsc::unbounded_channel};
use tracing::info;

use crate::{
    orchestrator::Orchestrator,
    recipes::RecipeRegistry,
    types::{ConversationSummary, ProgressEvent, RunTurnResult, UiEvent},
};

static INDEX_HTML: &str = include_str!("static/index.html");

// ---------------------------------------------------------------------------
// Job registry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct JobState {
    pub status: String,
    pub events: Vec<ProgressEvent>,
    pub result: Option<RunTurnResult>,
    pub error: Option<String>,
}

pub type JobRegistry = Arc<Mutex<HashMap<String, JobState>>>;

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

// ---------------------------------------------------------------------------
// Error helper
// ---------------------------------------------------------------------------

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = json!({"error": self.0.to_string()});
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(body),
        )
            .into_response()
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
    let convo = state.orchestrator.store().load(&id)?;
    Ok(axum::Json(serde_json::to_value(&convo)?))
}

async fn delete_conversation(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    state.orchestrator.store().delete(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn post_message(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<PostMessageBody>,
) -> AppResult<(StatusCode, axum::Json<serde_json::Value>)> {
    let job_id = new_job_id();
    {
        let mut jobs = state.jobs.lock().await;
        jobs.insert(
            job_id.clone(),
            JobState {
                status: "running".to_string(),
                events: Vec::new(),
                result: None,
                error: None,
            },
        );
    }

    let orchestrator = state.orchestrator.clone();
    let jobs = state.jobs.clone();
    let conversation_id = id.clone();
    let job_id_clone = job_id.clone();
    let content = body.content;

    tokio::spawn(async move {
        let (ui_tx, mut ui_rx) = unbounded_channel::<UiEvent>();

        // Collect events in parallel while running
        let jobs_ev = jobs.clone();
        let job_id_ev = job_id_clone.clone();
        tokio::spawn(async move {
            while let Some(event) = ui_rx.recv().await {
                if let UiEvent::Progress(p) = event {
                    let mut lock = jobs_ev.lock().await;
                    if let Some(job) = lock.get_mut(&job_id_ev) {
                        job.events.push(p);
                    }
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
                    job.status = "done".to_string();
                    job.result = Some(r);
                }
                Err(e) => {
                    job.status = "failed".to_string();
                    job.error = Some(format!("{e:#}"));
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
) -> AppResult<axum::Json<serde_json::Value>> {
    let (ui_tx, _ui_rx) = unbounded_channel::<UiEvent>();
    let summary = state
        .orchestrator
        .compact_conversation(&id, ui_tx)
        .await?;
    Ok(axum::Json(json!({"summary": summary})))
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
    let convo = state.orchestrator.store().load(&id)?;
    Ok(axum::Json(json!({"enabled": convo.enabled_mcp_servers})))
}

async fn put_mcp_servers(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    axum::Json(body): axum::Json<McpServersBody>,
) -> AppResult<axum::Json<serde_json::Value>> {
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
    let store = state.orchestrator.store();
    let mut convo = store.load(&id)?;
    convo.enabled_mcp_servers = None;
    store.save(&convo)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_job(
    State(state): State<WebAppState>,
    Path(job_id): Path<String>,
) -> AppResult<axum::Json<serde_json::Value>> {
    let jobs = state.jobs.lock().await;
    let job = jobs
        .get(&job_id)
        .ok_or_else(|| anyhow::anyhow!("job '{job_id}' not found"))?;
    Ok(axum::Json(serde_json::to_value(job)?))
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

    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/api/conversations", get(list_conversations).post(create_conversation))
        .route("/api/conversations/{id}", get(get_conversation).delete(delete_conversation))
        .route("/api/conversations/{id}/messages", post(post_message))
        .route("/api/conversations/{id}/compact", post(compact_conversation))
        .route("/api/conversations/{id}/recipe", post(use_recipe).delete(clear_recipe))
        .route("/api/conversations/{id}/mcp-servers", get(get_mcp_servers).put(put_mcp_servers).delete(delete_mcp_servers))
        .route("/api/recipes", get(list_recipes))
        .route("/api/jobs/{job_id}", get(get_job))
        .with_state(state);

    let addr = format!("{host}:{port}");
    info!(addr, "starting web server");
    println!("Web interface running at http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
