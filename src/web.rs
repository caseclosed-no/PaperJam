use crate::{model::*, planner, printer, state::AppState, store};
use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;
const INDEX: &str = include_str!("../static/index.html");
const JS: &str = include_str!("../static/app.js");
const CSS: &str = include_str!("../static/styles.css");
pub async fn index() -> Html<&'static str> {
    Html(INDEX)
}
pub async fn javascript() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        JS,
    )
}
pub async fn styles() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS)
}
pub async fn paperjam_mark() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/svg+xml")],
        include_str!("../static/paperjam-mark.svg"),
    )
}
pub async fn paperjam_logo() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/svg+xml")],
        include_str!("../static/paperjam-logo.svg"),
    )
}
pub async fn caseclosed_logo() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/png")],
        &include_bytes!("../static/caseclosed-logo.png")[..],
    )
}
pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok"}))
}
#[derive(Serialize)]
pub struct Catalog {
    games: Vec<Game>,
    profiles: BTreeMap<String, PrintProfile>,
    backend: String,
    destination: String,
    warnings: Vec<String>,
}
pub async fn catalog(State(s): State<AppState>) -> Json<Catalog> {
    Json(Catalog {
        games: s.games.as_ref().clone(),
        profiles: s.printer.profiles.clone(),
        backend: s.printer.backend.clone(),
        destination: s.printer.destination.clone(),
        warnings: s.warnings.as_ref().clone(),
    })
}
pub async fn get_inventory(State(s): State<AppState>) -> Json<InventoryFile> {
    Json(s.inventory.read().await.clone())
}
pub async fn put_inventory(
    State(s): State<AppState>,
    Json(req): Json<InventoryUpdate>,
) -> Result<Json<InventoryFile>, ApiError> {
    let mut inv = s.inventory.write().await;
    if inv.revision != req.expected_revision {
        return Err(ApiError::conflict(
            "inventory changed; reload before saving",
        ));
    }
    inv.counts = req.counts;
    inv.revision += 1;
    crate::inventory::save(&s.state_directory, &inv).map_err(ApiError::internal)?;
    Ok(Json(inv.clone()))
}
pub async fn production_plan(
    State(s): State<AppState>,
    Json(req): Json<ProductionPlanRequest>,
) -> Result<Json<PlanResponse>, ApiError> {
    let (game, doc) = store::find_game_document(&s.games, &req.game_id, &req.document_id)
        .map_err(ApiError::bad)?;
    let inv = s.inventory.read().await;
    let plan = planner::production_plan(game, doc, &inv, &req).map_err(ApiError::bad)?;
    respond_plan(&s, doc, plan).await
}
pub async fn manual_plan(
    State(s): State<AppState>,
    Json(req): Json<ManualPlanRequest>,
) -> Result<Json<PlanResponse>, ApiError> {
    let (game, doc) = store::find_game_document(&s.games, &req.game_id, &req.document_id)
        .map_err(ApiError::bad)?;
    let inv = s.inventory.read().await;
    let plan = planner::manual_plan(game, doc, &inv, &req).map_err(ApiError::bad)?;
    respond_plan(&s, doc, plan).await
}
async fn respond_plan(
    s: &AppState,
    doc: &Document,
    plan: PrintPlan,
) -> Result<Json<PlanResponse>, ApiError> {
    let jobs = printer::compile_jobs(doc, &plan, &s.printer, &s.pdf_root).map_err(ApiError::bad)?;
    s.store_plan(plan.clone()).await;
    Ok(Json(PlanResponse { plan, jobs }))
}
pub async fn start_run(
    State(s): State<AppState>,
    Json(req): Json<StartRunRequest>,
) -> Result<(StatusCode, Json<PrintRun>), ApiError> {
    let run = s.start_run(req.plan_id).await.map_err(|e| {
        if e.to_string().contains("inventory changed") {
            ApiError::conflict(e)
        } else {
            ApiError::bad(e)
        }
    })?;
    Ok((StatusCode::ACCEPTED, Json(run)))
}
pub async fn list_runs(State(s): State<AppState>) -> Json<Vec<PrintRun>> {
    let mut runs: Vec<_> = s.runs.read().await.values().cloned().collect();
    runs.sort_by_key(|run| std::cmp::Reverse(run.created_at));
    Json(runs)
}
pub async fn get_run(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PrintRun>, ApiError> {
    s.runs
        .read()
        .await
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found("unknown run"))
}
pub async fn cancel_run(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PrintRun>, ApiError> {
    Ok(Json(s.cancel_run(id).await.map_err(ApiError::bad)?))
}
pub async fn restore_stock(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PrintRun>, ApiError> {
    Ok(Json(s.restore_stock(id).await.map_err(ApiError::bad)?))
}
pub async fn printer_status(
    State(s): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let status = printer::status(&s.printer)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(
        serde_json::json!({"status":status,"backend":s.printer.backend,"destination":s.printer.destination}),
    ))
}
pub struct ApiError(StatusCode, String);
impl ApiError {
    fn bad(e: impl ToString) -> Self {
        Self(StatusCode::BAD_REQUEST, e.to_string())
    }
    fn conflict(e: impl ToString) -> Self {
        Self(StatusCode::CONFLICT, e.to_string())
    }
    fn internal(e: impl ToString) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
    fn not_found(e: impl ToString) -> Self {
        Self(StatusCode::NOT_FOUND, e.to_string())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
