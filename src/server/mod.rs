pub mod handlers;

use axum::{Router, routing::get};
use std::sync::Arc;
use tower_http::services::ServeDir;
use crate::data::spatial::TileIndex;
use crate::data::mmap::MappedData;
use crate::labels::PlaceLabelStore;
use crate::renderer::ShaderType;
use handlers::handle_tile_request;

#[derive(Clone)]
pub struct AppState {
    pub data: Arc<TileIndex>,
    pub mmap: Arc<MappedData>,
    pub low_zoom_data: Option<Arc<MappedData>>,
    pub low_zoom_index: Option<Arc<TileIndex>>,
    pub low_zoom_max: u32,
    pub labels: Arc<PlaceLabelStore>,
    pub shader_type: ShaderType,
    pub stylesheet: Option<String>,
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/tile/:z/:x/:y.png", get(handle_tile_request))
        .nest_service("/", ServeDir::new("static"))
        .with_state(state)
}
