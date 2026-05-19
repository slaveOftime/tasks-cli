use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(debug_assertions)]
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Form, Json, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{TaskRecord, TaskSchedule, TaskStatus, TaskSummary};
use crate::service::{
    AddNoteRequest, AddTaskRequest, BlockTaskRequest, ContinuationQuery, DependencyTaskRequest,
    DoneTaskRequest, EventsQuery, NoteRequest, ProgressRequest, ReadyQuery, ScheduleTaskRequest,
    StateQuery, TaskListQuery, TaskService,
};

const BOARD_PAGE_SIZE: usize = 15;

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

struct UiPaths;

impl UiPaths {
    const APP_CSS: &str = "assets/app.css";
    const HTMX_JS: &str = "assets/htmx.js";
    const APP_JS: &str = "assets/app.js";
    const BOARD: &str = "ui/board";
    const TASKS: &str = "ui/tasks";

    fn task_action(id: &str, action: &str) -> String {
        format!("ui/tasks/{id}/{action}")
    }

    fn dependency_action(id: &str, action: &str) -> String {
        format!("ui/tasks/{id}/dependencies/{action}")
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ServerOptions {
    pub(crate) port: u16,
}

#[derive(Clone)]
struct AppState {
    service: TaskService,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct BoardQuery {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    ready_page: Option<usize>,
    #[serde(default)]
    todo_page: Option<usize>,
    #[serde(default)]
    active_page: Option<usize>,
    #[serde(default)]
    checkpoint_page: Option<usize>,
    #[serde(default)]
    blocked_page: Option<usize>,
    #[serde(default)]
    review_page: Option<usize>,
    #[serde(default)]
    done_page: Option<usize>,
}

impl BoardQuery {
    fn search_query(&self) -> Option<&str> {
        self.query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn page_for(&self, class_name: &str) -> usize {
        let page = match class_name {
            "ready" => self.ready_page,
            "todo" => self.todo_page,
            "active" => self.active_page,
            "checkpoint" => self.checkpoint_page,
            "blocked" => self.blocked_page,
            "review" => self.review_page,
            "done" => self.done_page,
            _ => None,
        };
        page.unwrap_or(1).max(1)
    }

    fn query_string_for(&self, class_name: &str, page: usize) -> String {
        let mut params = Vec::new();
        if let Some(query) = self.search_query() {
            push_query_param(&mut params, "query", query);
        }
        for (name, value) in [
            ("ready_page", self.page_override("ready", class_name, page)),
            ("todo_page", self.page_override("todo", class_name, page)),
            (
                "active_page",
                self.page_override("active", class_name, page),
            ),
            (
                "checkpoint_page",
                self.page_override("checkpoint", class_name, page),
            ),
            (
                "blocked_page",
                self.page_override("blocked", class_name, page),
            ),
            (
                "review_page",
                self.page_override("review", class_name, page),
            ),
            ("done_page", self.page_override("done", class_name, page)),
        ] {
            if value > 1 {
                push_query_param(&mut params, name, &value.to_string());
            }
        }
        params.join("&")
    }

    fn page_override(&self, column: &str, selected: &str, selected_page: usize) -> usize {
        if column == selected {
            selected_page.max(1)
        } else {
            self.page_for(column)
        }
    }
}

struct ColumnPagination {
    page: usize,
    total_pages: usize,
    total_items: usize,
    start_index: usize,
    end_index: usize,
}

impl ColumnPagination {
    fn for_total(requested_page: usize, total_items: usize) -> Self {
        let total_pages = if total_items == 0 {
            1
        } else {
            ((total_items - 1) / BOARD_PAGE_SIZE) + 1
        };
        let page = requested_page.max(1).min(total_pages);
        let start_index = total_items.min((page - 1) * BOARD_PAGE_SIZE);
        let end_index = total_items.min(start_index + BOARD_PAGE_SIZE);
        Self {
            page,
            total_pages,
            total_items,
            start_index,
            end_index,
        }
    }

    fn has_previous(&self) -> bool {
        self.page > 1
    }

    fn has_next(&self) -> bool {
        self.page < self.total_pages
    }
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
    Html(render_index(&state.service))
}

fn render_index(service: &TaskService) -> String {
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Tasks Kanban</title>
  <link rel="stylesheet" href="{}">
  <script defer src="{}"></script>
  <script defer src="{}"></script>
</head>
<body>
  <header id="page-top" class="topbar">
    <div class="topbar__brand">
      <h1>Tasks Kanban</h1>
    </div>
    <div class="topbar__actions">
      <div class="root" title="Store root">{}</div>
      <button type="button" data-dialog-open="create-task-dialog">Create task</button>
    </div>
  </header>
  <main id="board" hx-get="{}" hx-trigger="load" hx-swap="outerHTML">
    <section class="loading">Loading task board...</section>
  </main>
</body>
</html>"##,
        UiPaths::APP_CSS,
        UiPaths::HTMX_JS,
        UiPaths::APP_JS,
        escape_html(&service.root().display().to_string()),
        UiPaths::BOARD
    )
}

async fn asset_css() -> Response {
    asset_response("text/css; charset=utf-8", "app.css", APP_CSS)
}

async fn asset_htmx() -> Response {
    asset_response("application/javascript; charset=utf-8", "htmx.js", HTMX_JS)
}

async fn asset_js() -> Response {
    asset_response("application/javascript; charset=utf-8", "app.js", APP_JS)
}

fn asset_response(content_type: &'static str, filename: &str, embedded: &'static str) -> Response {
    match asset_text(filename, embedded) {
        Ok(body) => ([(header::CONTENT_TYPE, content_type)], body).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!("failed to read asset '{filename}': {error}"),
        )
            .into_response(),
    }
}

#[cfg(debug_assertions)]
fn asset_text(filename: &str, embedded: &'static str) -> std::io::Result<String> {
    asset_text_from_path(&debug_asset_path(filename), embedded)
}

#[cfg(not(debug_assertions))]
fn asset_text(_filename: &str, embedded: &'static str) -> std::io::Result<String> {
    Ok(embedded.to_string())
}

#[cfg(debug_assertions)]
fn asset_text_from_path(path: &FsPath, _embedded: &'static str) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

#[cfg(debug_assertions)]
fn debug_asset_path(filename: &str) -> PathBuf {
    FsPath::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("server_assets")
        .join(filename)
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

async fn ui_board(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BoardQuery>,
) -> UiResult<Html<String>> {
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_add_task(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<AddTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.add_task(input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_start_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<NoteRequest>,
) -> UiResult<Html<String>> {
    state.service.start_task(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_checkpoint_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<ProgressRequest>,
) -> UiResult<Html<String>> {
    state.service.checkpoint_task(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_block_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<BlockTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.block_task(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_review_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<NoteRequest>,
) -> UiResult<Html<String>> {
    state.service.review_task(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_done_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<DoneTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.complete_task(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_add_note(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<AddNoteRequest>,
) -> UiResult<Html<String>> {
    state.service.add_note(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_schedule_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<ScheduleTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.schedule_task(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_add_dependency(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<DependencyTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.add_dependency(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

async fn ui_remove_dependency(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<BoardQuery>,
    Form(input): Form<DependencyTaskRequest>,
) -> UiResult<Html<String>> {
    state.service.remove_dependency(&id, input)?;
    render_board(&state.service, &query)
        .map(Html)
        .map_err(AppError::from)
}

fn render_board(service: &TaskService, query: &BoardQuery) -> Result<String> {
    let search_query = query.search_query().map(str::to_string);
    let snapshot = service.state_snapshot(StateQuery {
        query: search_query.clone(),
        limit: Some(12),
    })?;
    let tasks = service.list_tasks(TaskListQuery {
        all: Some(true),
        query: search_query.clone(),
        ..TaskListQuery::default()
    })?;
    let ready_ids = service
        .ready_tasks(ReadyQuery {
            query: search_query.clone(),
            ..ReadyQuery::default()
        })?
        .into_iter()
        .map(|task| task.task.id)
        .collect::<std::collections::BTreeSet<_>>();
    let create_task_path = board_action_path(UiPaths::TASKS, &query.query_string_for("todo", 1));
    let search_value = query.search_query().unwrap_or("");
    let search_results_summary = if search_value.is_empty() {
        None
    } else if tasks.is_empty() {
        Some(format!("No tasks match \"{search_value}\"."))
    } else if tasks.len() == 1 {
        Some(format!("1 matching task for \"{search_value}\"."))
    } else {
        Some(format!(
            "{} matching tasks for \"{search_value}\".",
            tasks.len()
        ))
    };
    let search_input_path = board_action_path(UiPaths::BOARD, "");
    let clear_search_button = if search_value.is_empty() {
        String::new()
    } else {
        format!(
            r##"<button type="button" class="secondary" hx-get="{}" hx-target="#board" hx-swap="outerHTML">Clear</button>"##,
            escape_html(UiPaths::BOARD)
        )
    };
    let search_results_summary = search_results_summary.map(|summary| {
        format!(
            r#"<p class="board-search__summary">{}</p>"#,
            escape_html(&summary)
        )
    });

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
    <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="create-form" data-ready-form>
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
<section class="board-toolbar">
  <form class="board-search" role="search" hx-get="{}" hx-target="#board" hx-swap="outerHTML">
    <label class="board-search__field">
      <input type="search" name="query" value="{}" placeholder="Search titles, ids, labels" aria-label="Search tasks" autocomplete="off">
    </label>
    <div class="board-search__actions">
      <button type="submit" class="secondary">Search</button>
      {}
    </div>
    {}
  </form>
</section>
<nav class="metrics" aria-label="Task status summary">"##,
        escape_html(&create_task_path),
        escape_html(&search_input_path),
        escape_html(search_value),
        clear_search_button,
        search_results_summary.unwrap_or_default(),
    )?;
    for (class_name, count) in [
        ("ready", snapshot.counts.ready),
        ("todo", snapshot.counts.todo),
        ("active", snapshot.counts.active),
        ("checkpoint", snapshot.counts.checkpoint),
        ("blocked", snapshot.counts.blocked),
        ("review", snapshot.counts.review),
        ("done", snapshot.counts.done),
    ] {
        render_status_summary_item(
            &mut html,
            status_display_label(class_name),
            class_name,
            count,
        )?;
    }
    write!(
        html,
        r##"</nav>
<section class="kanban">"##,
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
        render_column(&mut html, service, query, title, class_name, &column_tasks)?;
    }

    html.push_str(
        r#"</section>
<button type="button" class="scroll-top" data-scroll-top data-visible="false" aria-label="Scroll to top" title="Scroll to top">
  <span class="scroll-top__icon" aria-hidden="true">&uarr;</span>
</button>
</main>"#,
    );
    Ok(html)
}

fn render_column(
    html: &mut String,
    service: &TaskService,
    query: &BoardQuery,
    title: &str,
    class_name: &str,
    tasks: &[&TaskSummary],
) -> Result<()> {
    let section_id = status_section_id(class_name);
    let pagination = ColumnPagination::for_total(query.page_for(class_name), tasks.len());
    write!(
        html,
        r#"<section id="{}" class="column column-{}"><header><h2 class="column__title">{}</h2><span class="column__count">{}</span></header><div class="column-cards">"#,
        escape_html(&section_id),
        class_name,
        escape_html(title),
        tasks.len()
    )?;
    for task in tasks[pagination.start_index..pagination.end_index].iter() {
        render_task_card(html, service, query, task, class_name)?;
    }
    html.push_str("</div>");
    render_column_pagination(html, query, class_name, &pagination)?;
    html.push_str("</section>");
    Ok(())
}

fn render_task_card(
    html: &mut String,
    service: &TaskService,
    query: &BoardQuery,
    task: &TaskSummary,
    status_class_name: &str,
) -> Result<()> {
    let detail = service.task_detail(&task.id)?;
    let events = service.task_events(Some(&task.id), Some(3))?;
    let id = escape_html(&task.id);
    let title = escape_html(&task.title);
    write!(
        html,
        r#"<article class="task-card task-card--{}">
<div class="task-card__head">
  <div class="task-card__title-row">
    <h3>{}</h3>
    <div class="labels">"#,
        status_class_name, title
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
    write!(
        html,
        r#"</div><code class="task-card__id">{}</code></div>"#,
        id
    )?;
    if let Some(summary) = detail.task.summary_text.as_deref() {
        write!(html, r#"<p class="summary">{}</p>"#, escape_html(summary))?;
    }
    if let Some(reason) = detail.task.blocked_reason.as_deref() {
        render_detail_row(html, "blocked", reason, true)?;
    }
    if let Some(schedule) = &task.schedule {
        render_detail_row(html, "schedule", &schedule.to_string(), false)?;
    }
    if !task.depends_on.is_empty() {
        render_detail_row(html, "depends on", &task.depends_on.join(", "), false)?;
    }
    if !detail.blocked_by.is_empty() {
        let blocked_by = detail
            .blocked_by
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        render_detail_row(html, "blocked by", &blocked_by, true)?;
    }
    if !task.continuation.is_empty() {
        let mut parts = Vec::new();
        if let Some(step) = task.continuation.next_step.as_deref() {
            parts.push(format!("step: {step}"));
        }
        if let Some(next_task) = task.continuation.next_task.as_deref() {
            parts.push(format!("task: {next_task}"));
        }
        render_detail_row(html, "next", &parts.join("; "), false)?;
    }
    if !events.is_empty() {
        html.push_str(r#"<ul class="events">"#);
        for event in events {
            let event_label = format_card_timestamp(event.at);
            write!(
                html,
                r#"<li><div class="event-kind">{}<time class="event-time" datetime="{}">({})</time></div>"#,
                escape_html(&event.kind.to_string()),
                escape_html(&event.at.to_rfc3339()),
                escape_html(&event_label),
            )?;
            if !event.message.is_empty() {
                write!(
                    html,
                    r#"<div class="event-message">{}</div>"#,
                    escape_html(&event.message)
                )?;
            }
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }

    if task.status == TaskStatus::Done {
        html.push_str("</article>");
        return Ok(());
    }

    let start_disabled = disabled_attr(task.status == TaskStatus::Active);
    let review_disabled = disabled_attr(task.status == TaskStatus::Review);
    let done_disabled = disabled_attr(task.status == TaskStatus::Done);
    let status_query = query_string_for_status(task.status);
    let start_path = board_action_path(
        &UiPaths::task_action(&id, "start"),
        &query.query_string_for(status_query, 1),
    );
    let review_path = board_action_path(
        &UiPaths::task_action(&id, "review"),
        &query.query_string_for(status_query, 1),
    );
    let done_path = board_action_path(
        &UiPaths::task_action(&id, "done"),
        &query.query_string_for("done", query.page_for("done")),
    );
    let checkpoint_path = board_action_path(
        &UiPaths::task_action(&id, "checkpoint"),
        &query.query_string_for("checkpoint", query.page_for("checkpoint")),
    );
    let block_path = board_action_path(
        &UiPaths::task_action(&id, "block"),
        &query.query_string_for("blocked", query.page_for("blocked")),
    );
    let note_path = board_action_path(
        &UiPaths::task_action(&id, "note"),
        &query.query_string_for(status_query, query.page_for(status_query)),
    );
    let add_dependency_path = board_action_path(
        &UiPaths::dependency_action(&id, "add"),
        &query.query_string_for(status_query, query.page_for(status_query)),
    );
    let remove_dependency_path = board_action_path(
        &UiPaths::dependency_action(&id, "remove"),
        &query.query_string_for(status_query, query.page_for(status_query)),
    );
    let schedule_section = render_schedule_section(
        &id,
        task.schedule.as_ref(),
        task.ready_at,
        &query.query_string_for(status_query, query.page_for(status_query)),
    )?;
    write!(
        html,
        r##"<div class="actions">
  <form hx-post="{}" hx-target="#board" hx-swap="outerHTML"><button type="submit"{}>Start</button></form>
  <form hx-post="{}" hx-target="#board" hx-swap="outerHTML"><button type="submit"{}>Review</button></form>
  <form hx-post="{}" hx-target="#board" hx-swap="outerHTML"><button type="submit"{}>Done</button></form>
  <button type="button" class="secondary" data-dialog-open="manage-{}">Manage</button>
</div>
<dialog id="manage-{}" class="app-dialog">
  <div class="dialog-card">
    <header>
        <div>
        <p class="eyebrow">manage task</p>
        <h2>{}</h2>
        </div>
    </header>
    <button type="button" class="dialog-close" data-dialog-close aria-label="Close dialog">&times;</button>
    <div class="dialog-content">
        <div class="dialog-grid">
        <section class="dialog-section">
            <h3>Checkpoint</h3>
            <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="stack" data-ready-form data-ready-any>
            <input name="note" placeholder="checkpoint note">
            <input name="next_step" placeholder="next step">
            <input name="next_task" placeholder="next task id">
            <button type="submit" data-ready-submit hidden>Checkpoint</button>
            </form>
        </section>
        <section class="dialog-section">
            <h3>Block</h3>
            <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="stack" data-ready-form>
            <input name="reason" required placeholder="blocked reason">
            <button type="submit" data-ready-submit hidden>Block</button>
            </form>
        </section>
        <section class="dialog-section">
            <h3>Note</h3>
            <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="stack" data-ready-form>
            <input name="text" required placeholder="note">
            <button type="submit" data-ready-submit hidden>Add note</button>
            </form>
        </section>
        {}
        <section class="dialog-section">
            <h3>Dependencies</h3>
            <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="inline" data-ready-form>
            <input name="dependency" required placeholder="dependency id">
            <button type="submit" data-ready-submit hidden>Add dep</button>
            </form>
            <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="inline" data-ready-form>
            <input name="dependency" required placeholder="dependency id">
            <button type="submit" data-ready-submit hidden>Remove dep</button>
            </form>
        </section>
        </div>
    </div>
  </div>
</dialog>
</article>"##,
        escape_html(&start_path),
        start_disabled,
        escape_html(&review_path),
        review_disabled,
        escape_html(&done_path),
        done_disabled,
        id,
        id,
        title,
        escape_html(&checkpoint_path),
        escape_html(&block_path),
        escape_html(&note_path),
        schedule_section,
        escape_html(&add_dependency_path),
        escape_html(&remove_dependency_path)
    )?;
    Ok(())
}

fn render_detail_row(html: &mut String, label: &str, value: &str, danger: bool) -> Result<()> {
    let label_class = if label == "depends on" {
        "detail-row__label detail-row__label--warning"
    } else {
        "detail-row__label"
    };
    let value_class = if danger {
        "detail-row__value danger"
    } else {
        "detail-row__value"
    };
    write!(
        html,
        r#"<p class="meta detail-row"><span class="{}">{}</span><span class="{}">{}</span></p>"#,
        label_class,
        escape_html(label),
        value_class,
        escape_html(value)
    )?;
    Ok(())
}

fn render_column_pagination(
    html: &mut String,
    query: &BoardQuery,
    class_name: &str,
    pagination: &ColumnPagination,
) -> Result<()> {
    if pagination.total_pages <= 1 {
        return Ok(());
    }

    let range_label = if pagination.total_items == 0 {
        "0 tasks".to_string()
    } else {
        format!(
            "{}-{} of {}",
            pagination.start_index + 1,
            pagination.end_index,
            pagination.total_items
        )
    };
    let previous_path = board_action_path(
        UiPaths::BOARD,
        &query.query_string_for(class_name, pagination.page.saturating_sub(1).max(1)),
    );
    let next_path = board_action_path(
        UiPaths::BOARD,
        &query.query_string_for(class_name, pagination.page + 1),
    );
    write!(
        html,
        r##"<footer class="column-pagination"><span>Page {} of {} · {}</span><div class="column-pagination__actions"><button type="button" class="secondary" hx-get="{}" hx-target="#board" hx-swap="outerHTML"{}>Prev</button><button type="button" class="secondary" hx-get="{}" hx-target="#board" hx-swap="outerHTML"{}>Next</button></div></footer>"##,
        pagination.page,
        pagination.total_pages,
        escape_html(&range_label),
        escape_html(&previous_path),
        disabled_attr(!pagination.has_previous()),
        escape_html(&next_path),
        disabled_attr(!pagination.has_next())
    )?;
    Ok(())
}

fn render_status_summary_item(
    html: &mut String,
    label: &str,
    class_name: &str,
    count: usize,
) -> Result<()> {
    let section_id = status_section_id(class_name);
    write!(
        html,
        r##"<a class="metrics__link metrics__link--{}" href="#{}" aria-label="Jump to {} tasks"><span>{} <strong>{}</strong></span></a>"##,
        class_name,
        escape_html(&section_id),
        escape_html(class_name),
        escape_html(label),
        count
    )?;
    Ok(())
}

fn status_display_label(class_name: &str) -> &'static str {
    match class_name {
        "ready" => "Ready",
        "todo" => "Todo",
        "active" => "Active",
        "checkpoint" => "Checkpoint",
        "blocked" => "Blocked",
        "review" => "Review",
        "done" => "Done",
        _ => "Unknown",
    }
}

fn status_section_id(class_name: &str) -> String {
    format!("status-{class_name}")
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

fn format_datetime_local_value(value: DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

fn render_schedule_section(
    id: &str,
    schedule: Option<&TaskSchedule>,
    ready_at: Option<DateTime<Utc>>,
    query_string: &str,
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
        <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="stack schedule-form" data-schedule-form>
          <div class="toggle-group" role="radiogroup" aria-label="Schedule mode">
            <label><input type="radio" name="schedule_mode" value="interval"{}> Interval</label>
            <label><input type="radio" name="schedule_mode" value="cron"{}> Cron</label>
          </div>"##,
        escape_html(&board_action_path(
            &UiPaths::task_action(id, "schedule"),
            query_string
        )),
        interval_checked,
        cron_checked
    )?;
    let ready_at_value = ready_at
        .map(|value| escape_html(&format_datetime_local_value(value)))
        .unwrap_or_default();
    write!(
        html,
        r#"
          <div class="schedule-panel" data-schedule-panel="interval"{}>
            <input name="every_minutes" type="number" min="1" placeholder="every minutes" value="{}"{}>
          </div>
          <div class="schedule-panel" data-schedule-panel="cron"{}>
            <input name="cron" placeholder="cron expression" value="{}"{}>
          </div>
          <input name="ready_at" type="datetime-local" step="1" placeholder="optional ready at" aria-label="Next ready at" value="{}" data-schedule-ready-at>"#,
        interval_hidden,
        interval_value,
        interval_disabled,
        cron_hidden,
        cron_value,
        cron_disabled,
        ready_at_value
    )?;
    html.push_str(
        r#"
          <button type="submit" data-ready-submit hidden>Save schedule</button>
        </form>"#,
    );
    if schedule.is_some() {
        write!(
            html,
            r##"
        <form hx-post="{}" hx-target="#board" hx-swap="outerHTML" class="schedule-clear-form">
          <input type="hidden" name="clear" value="true">
          <button type="submit" class="secondary">Clear schedule</button>
        </form>"##,
            escape_html(&board_action_path(
                &UiPaths::task_action(id, "schedule"),
                query_string
            ))
        )?;
    }
    html.push_str(
        r#"
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

fn board_action_path(path: &str, query_string: &str) -> String {
    if query_string.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query_string}")
    }
}

fn push_query_param(params: &mut Vec<String>, name: &str, value: &str) {
    params.push(format!(
        "{}={}",
        encode_query_component(name),
        encode_query_component(value)
    ));
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => {
                write!(encoded, "%{byte:02X}").expect("writing to string cannot fail");
            }
        }
    }
    encoded
}

fn query_string_for_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::Active => "active",
        TaskStatus::Checkpoint => "checkpoint",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Review => "review",
        TaskStatus::Done => "done",
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{BoardQuery, UiPaths, render_board, render_index};
    use crate::service::{AddTaskRequest, TaskService};
    use crate::store::TaskStore;

    #[test]
    fn index_uses_relative_asset_and_board_paths() {
        let temp = TempDir::new().unwrap();
        let service = TaskService::new(TaskStore::new(temp.path().join(".tli")));
        let html = render_index(&service);

        assert!(html.contains(r#"href="assets/app.css""#));
        assert!(html.contains(r#"src="assets/htmx.js""#));
        assert!(html.contains(r#"src="assets/app.js""#));
        assert!(html.contains(r#"hx-get="ui/board""#));
        assert!(!html.contains(r#"href="/assets/app.css""#));
        assert!(!html.contains(r#"src="/assets/htmx.js""#));
        assert!(!html.contains(r#"src="/assets/app.js""#));
        assert!(!html.contains(r#"hx-get="/ui/board""#));
    }

    #[test]
    fn board_uses_relative_ui_action_paths() {
        let temp = TempDir::new().unwrap();
        let store_root = temp.path().join(".tli");
        fs::create_dir_all(&store_root).unwrap();
        let service = TaskService::new(TaskStore::new(store_root));
        service
            .add_task(AddTaskRequest {
                title: "Ship proxy-safe board".into(),
                id: Some("proxy-safe-board".into()),
                ..AddTaskRequest::default()
            })
            .unwrap();

        let html = render_board(&service, &BoardQuery::default()).unwrap();

        for expected in [
            format!(r#"hx-post="{}""#, UiPaths::TASKS),
            format!(
                r#"hx-post="{}""#,
                UiPaths::task_action("proxy-safe-board", "start")
            ),
            format!(
                r#"hx-post="{}""#,
                UiPaths::task_action("proxy-safe-board", "checkpoint")
            ),
            format!(
                r#"hx-post="{}""#,
                UiPaths::task_action("proxy-safe-board", "schedule")
            ),
            format!(
                r#"hx-post="{}""#,
                UiPaths::dependency_action("proxy-safe-board", "add")
            ),
            format!(
                r#"hx-post="{}""#,
                UiPaths::dependency_action("proxy-safe-board", "remove")
            ),
        ] {
            assert!(html.contains(&expected), "missing {expected}");
        }

        for unexpected in [
            r#"hx-post="/ui/tasks""#,
            r#"hx-post="/ui/tasks/proxy-safe-board/start""#,
            r#"hx-post="/ui/tasks/proxy-safe-board/checkpoint""#,
            r#"hx-post="/ui/tasks/proxy-safe-board/schedule""#,
            r#"hx-post="/ui/tasks/proxy-safe-board/dependencies/add""#,
            r#"hx-post="/ui/tasks/proxy-safe-board/dependencies/remove""#,
        ] {
            assert!(!html.contains(unexpected), "unexpected {unexpected}");
        }
    }

    #[test]
    fn board_renders_independent_column_pagination_and_preserves_relative_paths() {
        let temp = TempDir::new().unwrap();
        let store_root = temp.path().join(".tli");
        fs::create_dir_all(&store_root).unwrap();
        let service = TaskService::new(TaskStore::new(store_root));

        for index in 1..=17 {
            service
                .add_task(AddTaskRequest {
                    title: format!("Todo task {index}"),
                    id: Some(format!("todo-task-{index:02}")),
                    ready_at: Some("2099-01-01T00:00:00Z".into()),
                    ..AddTaskRequest::default()
                })
                .unwrap();
        }
        for index in 1..=16 {
            let id = format!("done-task-{index:02}");
            service
                .add_task(AddTaskRequest {
                    title: format!("Done task {index}"),
                    id: Some(id.clone()),
                    ..AddTaskRequest::default()
                })
                .unwrap();
            service.start_task(&id, Default::default()).unwrap();
            service.complete_task(&id, Default::default()).unwrap();
        }

        let html = render_board(
            &service,
            &BoardQuery {
                todo_page: Some(2),
                done_page: Some(2),
                ..BoardQuery::default()
            },
        )
        .unwrap();

        assert!(html.contains(r#"column-pagination"#));
        assert!(html.contains(r#"aria-label="Task status summary""#));
        assert!(html.contains(r##"href="#status-ready""##));
        assert!(html.contains(r##"href="#status-checkpoint""##));
        assert!(html.contains(r#"class="metrics__link metrics__link--ready""#));
        assert!(html.contains(r#"id="status-ready" class="column column-ready""#));
        assert!(html.contains(r#"data-scroll-top"#));
        assert!(html.contains(r#"hx-get="ui/board?done_page=2""#));
        assert!(html.contains(r#"hx-get="ui/board?todo_page=2""#));
        assert!(html.contains(r#"<code class="task-card__id">todo-task-01</code>"#));
        assert!(!html.contains(r#"class="status-chip""#));
        assert!(!html.contains(r#"<code class="task-card__id">todo-task-17</code>"#));
        assert!(html.contains(r#"<code class="task-card__id">done-task-01</code>"#));
        assert!(!html.contains(r#"status-chip--done"#));
        assert!(!html.contains(r#"<code class="task-card__id">done-task-16</code>"#));
        assert!(!html.contains(r#"ui/tasks/done-task-01/done?todo_page=2&amp;done_page=2"#));
        assert!(!html.contains(r#"data-dialog-open="manage-done-task-01""#));
        assert!(!html.contains(r#"hx-get="/ui/board?todo_page=1&done_page=2""#));
    }

    #[test]
    fn board_search_filters_tasks_and_preserves_query_in_actions() {
        let temp = TempDir::new().unwrap();
        let store_root = temp.path().join(".tli");
        fs::create_dir_all(&store_root).unwrap();
        let service = TaskService::new(TaskStore::new(store_root));
        service
            .add_task(AddTaskRequest {
                title: "Alpha release polish".into(),
                id: Some("alpha-release".into()),
                labels: Some("launch".into()),
                ..AddTaskRequest::default()
            })
            .unwrap();
        service
            .add_task(AddTaskRequest {
                title: "Beta cleanup".into(),
                id: Some("beta-cleanup".into()),
                ..AddTaskRequest::default()
            })
            .unwrap();

        let html = render_board(
            &service,
            &BoardQuery {
                query: Some("alpha release".into()),
                ..BoardQuery::default()
            },
        )
        .unwrap();

        assert!(html.contains(r#"role="search""#));
        assert!(html.contains(
            r##"<form class="board-search" role="search" hx-get="ui/board" hx-target="#board" hx-swap="outerHTML">"##
        ));
        assert!(html.contains(r#"name="query" value="alpha release""#));
        assert!(!html.contains(r#"hx-trigger="input changed delay:500ms, search""#));
        assert!(html.contains(
            r#"class="board-search__summary">1 matching task for &quot;alpha release&quot;."#
        ));
        assert!(html.contains(r#"<code class="task-card__id">alpha-release</code>"#));
        assert!(!html.contains(r#"class="status-chip""#));
        assert!(!html.contains(r#"<code class="task-card__id">beta-cleanup</code>"#));
        assert!(html.contains(r#"hx-post="ui/tasks?query=alpha%20release""#));
        assert!(html.contains(r#"ui/tasks/alpha-release/start?query=alpha%20release"#));
        assert!(!html.contains("Page 1 of 1"));
        assert!(!html.contains("Filter tasks across every column."));
    }

    #[test]
    fn board_hides_pagination_for_single_page_columns() {
        let temp = TempDir::new().unwrap();
        let store_root = temp.path().join(".tli");
        fs::create_dir_all(&store_root).unwrap();
        let service = TaskService::new(TaskStore::new(store_root));
        service
            .add_task(AddTaskRequest {
                title: "Only task".into(),
                id: Some("only-task".into()),
                ready_at: Some("2099-01-01T00:00:00Z".into()),
                ..AddTaskRequest::default()
            })
            .unwrap();

        let html = render_board(&service, &BoardQuery::default()).unwrap();

        assert!(!html.contains(r#"column-pagination"#));
        assert!(!html.contains("Page 1 of 1"));
        assert!(!html.contains(">Prev</button>"));
        assert!(!html.contains(">Next</button>"));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_assets_are_read_from_disk() {
        let temp = TempDir::new().unwrap();
        let asset_path = temp.path().join("app.css");
        fs::write(&asset_path, "body { color: red; }").unwrap();

        assert_eq!(
            super::asset_text_from_path(&asset_path, "embedded").unwrap(),
            "body { color: red; }"
        );
    }
}
