mod app;
mod inventory;
mod model;
mod planner;
mod printer;
mod state;
mod store;
mod web;
use state::AppState;
use std::path::PathBuf;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = store::load_app_config("data/app.toml")?;
    let games = store::load_games("data/games")?;
    let printer = store::load_printer_config("data/printer.toml")?;
    let pdf_root = PathBuf::from(&config.pdf_directory);
    let state_dir = PathBuf::from(&config.state_directory);
    let warnings = store::validate(&games, &printer, &pdf_root)?;
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    let state = AppState::new(games, printer, pdf_root, state_dir, warnings)?;
    let app = app::router(state);
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    println!("Paperjam listening on http://{}", config.bind);
    axum::serve(listener, app).await?;
    Ok(())
}
