use crate::data::types::Tile;
use crate::encoding::png::encode_png;
use crate::renderer::{VulkanRenderer, ShaderType};
use crate::renderer::pipeline::{TILE_SIZE, TILE_SIZE_2X};
use crate::server::AppState;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use std::sync::Mutex;

/// Handle tile request
/// Path: /tile/:z/:x/:y.png or /tile/:z/:x/:y@2x.png
pub async fn handle_tile_request(
    State(state): State<AppState>,
    Path((z, x, y_png)): Path<(u32, u32, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    // Check for @2x suffix for high-resolution tiles
    let (y, tile_size) = if let Some(y_str) = y_png.strip_suffix("@2x.png") {
        let y = y_str.parse::<u32>().ok().ok_or(StatusCode::BAD_REQUEST)?;
        (y, TILE_SIZE_2X)
    } else {
        let y = y_png
            .strip_suffix(".png")
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;
        (y, TILE_SIZE)
    };

    log::info!("Rendering tile {}/{}/{} at {}px", z, x, y, tile_size);

    let tile = Tile::new(x, y, z);

    // Thread-local renderers for both 256px and 512px tiles
    thread_local! {
        static RENDERER_256: Mutex<Option<VulkanRenderer>> = Mutex::new(None);
        static RENDERER_512: Mutex<Option<VulkanRenderer>> = Mutex::new(None);
    }

    // Choose the appropriate renderer based on tile size
    let image = if tile_size == TILE_SIZE_2X {
        RENDERER_512.with(|renderer_cell| {
            let mut renderer_opt = renderer_cell.lock().unwrap();

            // Initialize 512px renderer if not yet created
            if renderer_opt.is_none() {
                let max_points = state.data.max_points;
                match VulkanRenderer::new_with_tile_size(max_points, state.shader_type, TILE_SIZE_2X) {
                    Ok(renderer) => {
                        *renderer_opt = Some(renderer);
                    }
                    Err(e) => {
                        log::error!("Failed to create 512px Vulkan renderer: {}", e);
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
            }

            let renderer = renderer_opt.as_mut().unwrap();
            renderer
                .render_tile(&tile, &state.data, &state.mmap)
                .map_err(|e| {
                    log::error!("Failed to render 512px tile: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })
        })?
    } else {
        RENDERER_256.with(|renderer_cell| {
            let mut renderer_opt = renderer_cell.lock().unwrap();

            // Initialize 256px renderer if not yet created
            if renderer_opt.is_none() {
                let max_points = state.data.max_points;
                match VulkanRenderer::new(max_points, state.shader_type) {
                    Ok(renderer) => {
                        *renderer_opt = Some(renderer);
                    }
                    Err(e) => {
                        log::error!("Failed to create 256px Vulkan renderer: {}", e);
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
            }

            let renderer = renderer_opt.as_mut().unwrap();
            renderer
                .render_tile(&tile, &state.data, &state.mmap)
                .map_err(|e| {
                    log::error!("Failed to render 256px tile: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })
        })?
    };

    // Encode to PNG
    let png_data = encode_png(&image).map_err(|e| {
        log::error!("Failed to encode PNG: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(([(header::CONTENT_TYPE, "image/png")], png_data))
}
