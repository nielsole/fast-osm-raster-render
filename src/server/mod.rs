pub mod handlers;

use axum::{Router, routing::get};
use std::sync::Arc;
use tower_http::services::ServeDir;
use crate::data::spatial::TileIndex;
use crate::data::mmap::MappedData;
use crate::renderer::ShaderType;
use handlers::handle_tile_request;

#[derive(Clone)]
pub struct AppState {
    pub data: Arc<TileIndex>,
    pub mmap: Arc<MappedData>,
    pub shader_type: ShaderType,
    pub data_file_path: String, // Phase 0: Path to data file for MapCSS
    pub stylesheet: Option<String>, // Phase 0: Optional MapCSS stylesheet
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/tile/:z/:x/:y.png", get(handle_tile_request))
        .nest_service("/", ServeDir::new("static"))
        .with_state(state)
}
