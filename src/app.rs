use crate::{state::AppState, web};
use axum::{
    Router,
    routing::{get, post},
};
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(web::index))
        .route("/app.js", get(web::javascript))
        .route("/styles.css", get(web::styles))
        .route("/paperjam-mark.svg", get(web::paperjam_mark))
        .route("/paperjam-logo.svg", get(web::paperjam_logo))
        .route("/caseclosed-logo.png", get(web::caseclosed_logo))
        .route("/health", get(web::health))
        .route("/api/catalog", get(web::catalog))
        .route(
            "/api/inventory",
            get(web::get_inventory).put(web::put_inventory),
        )
        .route("/api/plans/production", post(web::production_plan))
        .route("/api/plans/manual", post(web::manual_plan))
        .route("/api/runs", get(web::list_runs).post(web::start_run))
        .route("/api/runs/{id}", get(web::get_run))
        .route("/api/runs/{id}/cancel", post(web::cancel_run))
        .route("/api/runs/{id}/restore-stock", post(web::restore_stock))
        .route("/api/printer/status", get(web::printer_status))
        .with_state(state)
}
