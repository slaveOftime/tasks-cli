use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Form, Json, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{DateTime, Local, Utc};
use serde::Serialize;

use crate::model::{TaskRecord, TaskSchedule, TaskStatus, TaskSummary};
use crate::service::{
    AddNoteRequest, AddTaskRequest, BlockTaskRequest, ContinuationQuery, DependencyTaskRequest,
    DoneTaskRequest, EventsQuery, NoteRequest, ProgressRequest, ReadyQuery, ScheduleTaskRequest,
    StateQuery, TaskListQuery, TaskService,
};

const APP_CSS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/server_assets/app.css"
));
const HTMX_JS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/server_assets/htmx.js"
));
const APP_JS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/server_assets/app.js"
));

#[derive(Debug, Clone)]
pub(crate) struct ServerOptions {
    pub(crate) port: u16,
}

#[derive(Clone)]
struct AppState {
    service: TaskService,
}

pub(crate) fn start_server(service: TaskService, options: ServerOptions) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new().context("failed to start tokio runtime")?;
    runtime.block_on(async move {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), options.port);
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("failed to bind http server at http://{addr}"))?;
        let local_addr = listener
            .local_addr()
            .context("failed to read bound server address")?;
        println!("tli server listening at http://{local_addr}");
        axum::serve(listener, app(service))
            .await
            .context("server stopped unexpectedly")
    })
}

fn app(service: TaskService) -> Router {
    let state = Arc::new(AppState { service });
    Router::new()
        .route("/", get(index))
        .route("/assets/app.css", get(asset_css))
        .route("/assets/htmx.js", get(asset_htmx))
        .route("/assets/app.js", get(asset_js))
        .route("/ui/board", get(ui_board))
        .route("/ui/tasks", post(ui_add_task))
        .route("/ui/tasks/{id}/start", post(ui_start_task))
        .route("/ui/tasks/{id}/checkpoint", post(ui_checkpoint_task))
        .route("/ui/tasks/{id}/block", post(ui_block_task))
        .route("/ui/tasks/{id}/review", post(ui_review_task))
        .route("/ui/tasks/{id}/done", post(ui_done_task))
        .route("/ui/tasks/{id}/note", post(ui_add_note))
        .route("/ui/tasks/{id}/schedule", post(ui_schedule_task))
        .route("/ui/tasks/{id}/dependencies/add", post(ui_add_dependency))
        .route(
            "/ui/tasks/{id}/dependencies/remove",
            post(ui_remove_dependency),
        )
        .route("/api/state", get(api_state))
        .route("/api/ready", get(api_ready))
        .route("/api/next", get(api_next))
        .route("/api/events", get(api_events))
        .route("/api/tasks", get(api_list_tasks).post(api_add_task))
        .route("/api/tasks/{id}", get(api_task_detail))
        .route("/api/tasks/{id}/events", get(api_task_events))
        .route("/api/tasks/{id}/start", post(api_start_task))
        .route("/api/tasks/{id}/checkpoint", post(api_checkpoint_task))
        .route("/api/tasks/{id}/block", post(api_block_task))
        .route("/api/tasks/{id}/review", post(api_review_task))
        .route("/api/tasks/{id}/done", post(api_done_task))
        .route("/api/tasks/{id}/note", post(api_add_note))
        .route("/api/tasks/{id}/schedule", post(api_schedule_task))
        .route("/api/tasks/{id}/dependencies", post(api_add_dependency))
        .route(
            "/api/tasks/{id}/dependencies/remove",
            post(api_remove_dependency),
        )
        .with_state(state)
}

async fn index(State(state): State<Arc<AppState>>) -> Html<String> {
    Html(format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>tli task board</title>
  <link rel="stylesheet" href="/assets/app.css">
  <script defer src="/assets/htmx.js"></script>
  <script defer src="/assets/app.js"></script>
</head>
<body>
  <header class="topbar">
    <div>
      <p class="eyebrow">repo-local task management</p>
      <h1>tli Kanban</h1>
    </div>
    <div class="topbar__actions">
      <div class="root" title="Store root">{}</div>
      <button type="button" data-dialog-open="create-task-dialog">Create task</button>
    </div>
  </header>
  <main id="board" hx-get="/ui/board" hx-trigger="load" hx-swap="outerHTML">
    <section class="loading">Loading task board...</section>
  </main>
</body>
</html>"##,
        escape_html(&state.service.root().display().to_string())
    ))
}

async fn asset_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS)
}

async fn asset_htmx() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        HTMX_JS,
    )
}

async fn asset_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        APP_JS,
    )
}

async fn api_state(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StateQuery>,
) -> ApiResult<Json<impl Serialize>> {
    Ok(Json(state.service.state_snapshot(query)?))
}

async fn api_ready(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReadyQuery>,
) -> ApiResult<Json<impl Serialize>> {
    Ok(Json(state.service.ready_tasks(query)?))
}

async fn api_next(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ContinuationQuery>,
) -> ApiResult<Json<impl Serialize>> {
    Ok(Json(state.service.continuation_tasks(query)?))
}

async fn api_list_tasks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TaskListQuery>,
) -> ApiResult<Json<impl Serialize>> {
    Ok(Json(state.service.list_tasks(query)?))
}

async fn api_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventsQuery>,
) -> ApiResult<Json<impl Serialize>> {
    Ok(Json(state.service.task_events(None, query.limit)?))
}

async fn api_add_task(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AddTaskRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.add_task(input)?))
}

async fn api_task_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<impl Serialize>> {
    Ok(Json(state.service.task_detail(&id)?))
}

async fn api_task_events(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> ApiResult<Json<impl Serialize>> {
    Ok(Json(state.service.task_events(Some(&id), query.limit)?))
}

async fn api_start_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<NoteRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.start_task(&id, input)?))
}

async fn api_checkpoint_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<ProgressRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.checkpoint_task(&id, input)?))
}

async fn api_block_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<BlockTaskRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.block_task(&id, input)?))
}

async fn api_review_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<NoteRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.review_task(&id, input)?))
}

async fn api_done_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<DoneTaskRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.complete_task(&id, input)?))
}

async fn api_add_note(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<AddNoteRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.add_note(&id, input)?))
}

async fn api_schedule_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<ScheduleTaskRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.schedule_task(&id, input)?))
}

async fn api_add_dependency(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<DependencyTaskRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.add_dependency(&id, input)?))
}

async fn api_remove_dependency(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<DependencyTaskRequest>,
) -> ApiResult<Json<TaskRecord>> {
    Ok(Json(state.service.remove_dependency(&id, input)?))
}

async fn ui_board(State(state): State<Arc<AppState>>) -> UiResult<Html<String>> {
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_add_task(
    State(state): State<Arc<AppState>>,
    Form(input): Form<AddTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.add_task(input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_start_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<NoteRequest>,
) -> UiResult<Html<String>> {
    state.service.start_task(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_checkpoint_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<ProgressRequest>,
) -> UiResult<Html<String>> {
    state.service.checkpoint_task(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_block_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<BlockTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.block_task(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_review_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<NoteRequest>,
) -> UiResult<Html<String>> {
    state.service.review_task(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_done_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<DoneTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.complete_task(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_add_note(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<AddNoteRequest>,
) -> UiResult<Html<String>> {
    state.service.add_note(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_schedule_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<ScheduleTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.schedule_task(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_add_dependency(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<DependencyTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.add_dependency(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_remove_dependency(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(input): Form<DependencyTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.remove_dependency(&id, input)?;
    render_board(&state.service)
        .map(Html)
        .map_err(AppError::from)
}

fn render_board(service: &TaskService) -> Result<String> {
    let snapshot = service.state_snapshot(StateQuery {
        limit: Some(12),
        ..StateQuery::default()
    })?;
    let tasks = service.list_tasks(TaskListQuery {
        all: Some(true),
        limit: Some(500),
        ..TaskListQuery::default()
    })?;
    let ready_ids = service
        .ready_tasks(ReadyQuery {
            limit: Some(500),
            ..ReadyQuery::default()
        })?
        .into_iter()
        .map(|task| task.task.id)
        .collect::<std::collections::BTreeSet<_>>();

    let mut html = String::new();
    write!(
        html,
        r##"<main id="board" class="board-shell">
<dialog id="create-task-dialog" class="app-dialog">
  <div class="dialog-card">
    <header>
      <div>
        <p class="eyebrow">new task</p>
        <h2>Create task</h2>
      </div>
      <button type="button" class="dialog-close" data-dialog-close aria-label="Close dialog">&times;</button>
    </header>
    <form hx-post="/ui/tasks" hx-target="#board" hx-swap="outerHTML" class="create-form" data-ready-form>
      <input name="title" required placeholder="New task title">
      <input name="id" placeholder="optional-id">
      <input name="labels" placeholder="labels,comma-separated">
      <textarea name="summary" placeholder="Summary"></textarea>
      <div class="form-actions">
        <button type="submit" data-ready-submit hidden>Add task</button>
      </div>
    </form>
  </div>
</dialog>
<section class="metrics">
  <span>ready <strong>{}</strong></span>
  <span>todo <strong>{}</strong></span>
  <span>active <strong>{}</strong></span>
  <span>blocked <strong>{}</strong></span>
  <span>review <strong>{}</strong></span>
  <span>done <strong>{}</strong></span>
</section>
<section class="kanban">"##,
        snapshot.counts.ready,
        snapshot.counts.todo,
        snapshot.counts.active,
        snapshot.counts.blocked,
        snapshot.counts.review,
        snapshot.counts.done
    )?;

    let columns = [
        ("Ready", "ready", Vec::new()),
        ("Todo", "todo", vec![TaskStatus::Todo]),
        ("Active", "active", vec![TaskStatus::Active]),
        ("Checkpoint", "checkpoint", vec![TaskStatus::Checkpoint]),
        ("Blocked", "blocked", vec![TaskStatus::Blocked]),
        ("Review", "review", vec![TaskStatus::Review]),
        ("Done", "done", vec![TaskStatus::Done]),
    ];

    for (title, class_name, statuses) in columns {
        let column_tasks = tasks
            .iter()
            .filter(|task| {
                if class_name == "ready" {
                    ready_ids.contains(&task.id)
                } else if class_name == "todo" {
                    statuses.contains(&task.status) && !ready_ids.contains(&task.id)
                } else {
                    statuses.contains(&task.status)
                }
            })
            .collect::<Vec<_>>();
        render_column(&mut html, service, title, class_name, &column_tasks)?;
    }

    html.push_str("</section></main>");
    Ok(html)
}

fn render_column(
    html: &mut String,
    service: &TaskService,
    title: &str,
    class_name: &str,
    tasks: &[&TaskSummary],
) -> Result<()> {
    write!(
        html,
        r#"<section class="column column-{}"><header><h2>{}</h2><span>{}</span></header>"#,
        class_name,
        escape_html(title),
        tasks.len()
    )?;
    for task in tasks.iter().take(80) {
        render_task_card(html, service, task)?;
    }
    html.push_str("</section>");
    Ok(())
}

fn render_task_card(html: &mut String, service: &TaskService, task: &TaskSummary) -> Result<()> {
    let detail = service.task_detail(&task.id)?;
    let events = service.task_events(Some(&task.id), Some(3))?;
    let id = escape_html(&task.id);
    let title = escape_html(&task.title);
    write!(
        html,
        r#"<article class="task-card">
<div class="task-card__head">
  <h3>{}</h3>
  <div class="labels">"#,
        title
    )?;
    for label in &task.labels {
        write!(html, r#"<span>{}</span>"#, escape_html(label))?;
    }
    html.push_str("</div>");
    if let Some(ready_at) = task.ready_at {
        let ready_label = format_card_timestamp(ready_at);
        write!(
            html,
            r#"<time class="task-time" datetime="{}">{}</time>"#,
            escape_html(&ready_at.to_rfc3339()),
            escape_html(&ready_label)
        )?;
    }
    write!(html, r#"<code>{}</code></div>"#, id)?;
    if let Some(summary) = detail.task.summary_text.as_deref() {
        write!(html, r#"<p class="summary">{}</p>"#, escape_html(summary))?;
    }
    if let Some(reason) = detail.task.blocked_reason.as_deref() {
        write!(
            html,
            r#"<p class="meta danger">blocked: {}</p>"#,
            escape_html(reason)
        )?;
    }
    if let Some(schedule) = &task.schedule {
        write!(
            html,
            r#"<p class="meta">schedule {}</p>"#,
            escape_html(&schedule.to_string())
        )?;
    }
    if !task.depends_on.is_empty() {
        write!(
            html,
            r#"<p class="meta">depends on {}</p>"#,
            escape_html(&task.depends_on.join(", "))
        )?;
    }
    if !detail.blocked_by.is_empty() {
        let blocked_by = detail
            .blocked_by
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        write!(
            html,
            r#"<p class="meta danger">blocked by {}</p>"#,
            escape_html(&blocked_by)
        )?;
    }
    if !task.continuation.is_empty() {
        let mut parts = Vec::new();
        if let Some(step) = task.continuation.next_step.as_deref() {
            parts.push(format!("step: {step}"));
        }
        if let Some(next_task) = task.continuation.next_task.as_deref() {
            parts.push(format!("task: {next_task}"));
        }
        write!(
            html,
            r#"<p class="meta">next {}</p>"#,
            escape_html(&parts.join("; "))
        )?;
    }
    if !events.is_empty() {
        html.push_str(r#"<ul class="events">"#);
        for event in events {
            write!(
                html,
                "<li><span>{}</span> {}</li>",
                event.kind,
                escape_html(&event.message)
            )?;
        }
        html.push_str("</ul>");
    }
    let start_disabled = disabled_attr(task.status == TaskStatus::Active);
    let review_disabled = disabled_attr(task.status == TaskStatus::Review);
    let done_disabled = disabled_attr(task.status == TaskStatus::Done);
    let schedule_section = render_schedule_section(&id, task.schedule.as_ref(), task.ready_at)?;
    write!(
        html,
        r##"<div class="actions">
  <form hx-post="/ui/tasks/{}/start" hx-target="#board" hx-swap="outerHTML"><button type="submit"{}>Start</button></form>
  <form hx-post="/ui/tasks/{}/review" hx-target="#board" hx-swap="outerHTML"><button type="submit"{}>Review</button></form>
  <form hx-post="/ui/tasks/{}/done" hx-target="#board" hx-swap="outerHTML"><button type="submit"{}>Done</button></form>
  <button type="button" class="secondary" data-dialog-open="manage-{}">Manage</button>
</div>
<dialog id="manage-{}" class="app-dialog">
  <div class="dialog-card">
    <header>
      <div>
        <p class="eyebrow">manage task</p>
        <h2>{}</h2>
      </div>
      <button type="button" class="dialog-close" data-dialog-close aria-label="Close dialog">&times;</button>
    </header>
    <div class="dialog-grid">
      <section class="dialog-section">
        <h3>Checkpoint</h3>
        <form hx-post="/ui/tasks/{}/checkpoint" hx-target="#board" hx-swap="outerHTML" class="stack" data-ready-form data-ready-any>
          <input name="note" placeholder="checkpoint note">
          <input name="next_step" placeholder="next step">
          <input name="next_task" placeholder="next task id">
          <button type="submit" data-ready-submit hidden>Checkpoint</button>
        </form>
      </section>
      <section class="dialog-section">
        <h3>Block</h3>
        <form hx-post="/ui/tasks/{}/block" hx-target="#board" hx-swap="outerHTML" class="stack" data-ready-form>
          <input name="reason" required placeholder="blocked reason">
          <button type="submit" data-ready-submit hidden>Block</button>
        </form>
      </section>
      <section class="dialog-section">
        <h3>Note</h3>
        <form hx-post="/ui/tasks/{}/note" hx-target="#board" hx-swap="outerHTML" class="stack" data-ready-form>
          <input name="text" required placeholder="note">
          <button type="submit" data-ready-submit hidden>Add note</button>
        </form>
      </section>
      {}
      <section class="dialog-section">
        <h3>Dependencies</h3>
        <form hx-post="/ui/tasks/{}/dependencies/add" hx-target="#board" hx-swap="outerHTML" class="inline" data-ready-form>
          <input name="dependency" required placeholder="dependency id">
          <button type="submit" data-ready-submit hidden>Add dep</button>
        </form>
        <form hx-post="/ui/tasks/{}/dependencies/remove" hx-target="#board" hx-swap="outerHTML" class="inline" data-ready-form>
          <input name="dependency" required placeholder="dependency id">
          <button type="submit" data-ready-submit hidden>Remove dep</button>
        </form>
      </section>
    </div>
  </div>
</dialog>
</article>"##,
        id,
        start_disabled,
        id,
        review_disabled,
        id,
        done_disabled,
        id,
        id,
        title,
        id,
        id,
        id,
        schedule_section,
        id,
        id
    )?;
    Ok(())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn format_card_timestamp(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%b %d %H:%M")
        .to_string()
}

fn render_schedule_section(
    id: &str,
    schedule: Option<&TaskSchedule>,
    ready_at: Option<DateTime<Utc>>,
) -> Result<String> {
    let mut html = String::new();
    let (interval_checked, cron_checked, interval_hidden, cron_hidden, interval_value, cron_value) =
        match schedule {
            Some(TaskSchedule::Interval { every_minutes }) => (
                " checked",
                "",
                "",
                " hidden",
                every_minutes.to_string(),
                String::new(),
            ),
            Some(TaskSchedule::Cron { expression }) => (
                "",
                " checked",
                " hidden",
                "",
                String::new(),
                escape_html(expression),
            ),
            None => (" checked", "", "", " hidden", String::new(), String::new()),
        };
    let interval_disabled = if interval_hidden.is_empty() {
        ""
    } else {
        " disabled"
    };
    let cron_disabled = if cron_hidden.is_empty() {
        ""
    } else {
        " disabled"
    };

    write!(
        html,
        r##"<section class="dialog-section">
        <h3>Schedule</h3>
        <form hx-post="/ui/tasks/{}/schedule" hx-target="#board" hx-swap="outerHTML" class="stack schedule-form" data-schedule-form>
          <div class="toggle-group" role="radiogroup" aria-label="Schedule mode">
            <label><input type="radio" name="schedule_mode" value="interval"{}> Interval</label>
            <label><input type="radio" name="schedule_mode" value="cron"{}> Cron</label>"##,
        id, interval_checked, cron_checked
    )?;
    if schedule.is_some() {
        html.push_str(
            r#"
            <label><input type="radio" name="schedule_mode" value="clear"> Clear</label>"#,
        );
    }
    let ready_at_value = ready_at
        .map(|value| escape_html(&value.to_rfc3339()))
        .unwrap_or_default();
    write!(
        html,
        r#"
          </div>
          <div class="schedule-panel" data-schedule-panel="interval"{}>
            <input name="every_minutes" type="number" min="1" placeholder="every minutes" value="{}"{}>
          </div>
          <div class="schedule-panel" data-schedule-panel="cron"{}>
            <input name="cron" placeholder="cron expression" value="{}"{}>
          </div>
          <input name="ready_at" placeholder="optional ready at" value="{}" data-schedule-ready-at>"#,
        interval_hidden,
        interval_value,
        interval_disabled,
        cron_hidden,
        cron_value,
        cron_disabled,
        ready_at_value
    )?;
    if schedule.is_some() {
        html.push_str(
            r#"
          <div class="schedule-panel" data-schedule-panel="clear" hidden>
            <input type="hidden" name="clear" value="true" disabled>
            <p class="meta">Remove the recurring schedule and pending ready time.</p>
          </div>"#,
        );
    }
    html.push_str(
        r#"
          <button type="submit" data-ready-submit hidden>Save schedule</button>
        </form>
      </section>"#,
    );
    Ok(html)
}

fn disabled_attr(disabled: bool) -> &'static str {
    if disabled {
        r#" disabled aria-disabled="true""#
    } else {
        ""
    }
}

type ApiResult<T> = std::result::Result<T, AppError>;
type UiResult<T> = std::result::Result<T, AppError>;

struct AppError(anyhow::Error);

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let message = self.0.to_string();
        if message.contains("does not exist") {
            return (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                message,
            )
                .into_response();
        }
        (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            message,
        )
            .into_response()
    }
}
