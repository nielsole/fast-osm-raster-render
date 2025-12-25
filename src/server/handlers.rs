use crate::data::types::Tile;
use crate::encoding::png::encode_png;
use crate::renderer::{VulkanRenderer, ShaderType};
use crate::server::AppState;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use std::sync::Mutex;

/// Handle tile request
/// Path: /tile/:z/:x/:y.png
pub async fn handle_tile_request(
    State(state): State<AppState>,
    Path((z, x, y_png)): Path<(u32, u32, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    // Parse y coordinate and validate .png extension
    let y = y_png
        .strip_suffix(".png")
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    log::info!("Rendering tile {}/{}/{}", z, x, y);

    let tile = Tile::new(x, y, z);

    // Get renderer from thread-local storage or create new one
    // For now, we'll use a global mutex-protected renderer (simple v1)
    // TODO: Implement proper thread pool with one renderer per thread
    thread_local! {
        static RENDERER: Mutex<Option<VulkanRenderer>> = Mutex::new(None);
    }

    let image = RENDERER.with(|renderer_cell| {
        let mut renderer_opt = renderer_cell.lock().unwrap();

        // Initialize renderer if not yet created
        if renderer_opt.is_none() {
            let max_points = state.data.max_points;
            match VulkanRenderer::new(max_points, state.shader_type) {
                Ok(renderer) => {
                    *renderer_opt = Some(renderer);
                }
                Err(e) => {
                    log::error!("Failed to create Vulkan renderer: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }

        let renderer = renderer_opt.as_mut().unwrap();

        // Render tile
        renderer
            .render_tile(&tile, &state.data, &state.mmap)
            .map_err(|e| {
                log::error!("Failed to render tile: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })
    })?;

    // Encode to PNG
    let png_data = encode_png(&image).map_err(|e| {
        log::error!("Failed to encode PNG: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(([(header::CONTENT_TYPE, "image/png")], png_data))
}
