//! tasks — the "everything" service of this workspace: a small Tasks CRUD API
//! that wires every shared data lib together. Persistence is Valkey
//! ([`valkey`] — no relational database in this workspace), writes publish a
//! `task.created` event to Kafka ([`kafka`]), requests are traced ([`otelx`])
//! and errors are RFC 9457 [`httpx::Problem`] responses. `crates/consumer`
//! drains the events this service produces.
//!
//! Split into a lib so the routes are testable against the assembled router
//! without binding a socket (via `tower::ServiceExt::oneshot`).

#![allow(
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

mod config;
mod domain;
mod store;

pub use config::Config;
pub use domain::{Task, TaskCreatedEvent};
pub use store::TaskStore;

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use httpx::Problem;
use kafka::Producer;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

/// Everything the handlers need. Cheap to [`Clone`] (all handles are shared).
#[derive(Clone)]
pub struct AppState {
    pub store: TaskStore,
    pub producer: Producer,
    pub topic: String,
}

/// Build the tasks routes, to be merged onto an [`httpx::Server`].
pub fn routes(state: AppState) -> Router {
    Router::new()
        .route("/tasks", get(list).post(create))
        .route("/tasks/{id}", get(get_one).delete(delete_one))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct CreateTask {
    #[serde(default)]
    title: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn not_found(id: &str) -> Problem {
    Problem::new(StatusCode::NOT_FOUND, format!("no task with id {id}"))
        .with_type("https://rust-basics/errors/task-not-found")
        .with_title("Task not found")
        .with_instance(format!("/tasks/{id}"))
        .with_extension("code", "task_not_found")
}

/// `POST /tasks` — persist a new task, publish a `task.created` event and return
/// it. The event is best-effort: a task is durable once stored, so a broker
/// hiccup logs a warning rather than failing the request (production would close
/// that gap with a transactional outbox).
async fn create(
    State(state): State<AppState>,
    body: Result<Json<CreateTask>, JsonRejection>,
) -> Response {
    let Ok(Json(req)) = body else {
        return Problem::new(StatusCode::BAD_REQUEST, "invalid JSON body").into_response();
    };
    if req.title.is_empty() {
        return Problem::new(StatusCode::BAD_REQUEST, "task title must not be empty")
            .with_extension("code", "empty_title")
            .into_response();
    }

    let task = Task {
        id: Uuid::new_v4().to_string(),
        title: req.title,
        done: false,
        created_at: now_secs(),
    };
    if let Err(error) = state.store.create(&task).await {
        tracing::error!(%error, "could not persist task");
        return Problem::new(StatusCode::INTERNAL_SERVER_ERROR, "could not persist task")
            .into_response();
    }

    publish_created(&state, &task).await;
    (StatusCode::CREATED, Json(task)).into_response()
}

async fn publish_created(state: &AppState, task: &Task) {
    let event = TaskCreatedEvent::from(task);
    match serde_json::to_vec(&event) {
        Ok(payload) => {
            if let Err(error) = state
                .producer
                .publish(&state.topic, task.id.as_bytes(), &payload)
                .await
            {
                tracing::warn!(%error, id = %task.id, "event publish failed");
            }
        }
        Err(error) => tracing::warn!(%error, id = %task.id, "event marshal failed"),
    }
}

/// `GET /tasks/{id}` — fetch a task, or a 404 problem when it does not exist.
async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get(&id).await {
        Ok(Some(task)) => Json(task).into_response(),
        Ok(None) => not_found(&id).into_response(),
        Err(error) => {
            tracing::error!(%error, "could not load task");
            Problem::new(StatusCode::INTERNAL_SERVER_ERROR, "could not load task").into_response()
        }
    }
}

/// `GET /tasks` — list every task, newest first.
async fn list(State(state): State<AppState>) -> Response {
    match state.store.list().await {
        Ok(tasks) => Json(json!({ "tasks": tasks })).into_response(),
        Err(error) => {
            tracing::error!(%error, "could not list tasks");
            Problem::new(StatusCode::INTERNAL_SERVER_ERROR, "could not list tasks").into_response()
        }
    }
}

/// `DELETE /tasks/{id}` — remove a task, or a 404 problem when it does not exist.
async fn delete_one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.delete(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(&id).into_response(),
        Err(error) => {
            tracing::error!(%error, "could not delete task");
            Problem::new(StatusCode::INTERNAL_SERVER_ERROR, "could not delete task").into_response()
        }
    }
}
