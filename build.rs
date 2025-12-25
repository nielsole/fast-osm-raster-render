use std::error::Error;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    // Tell Cargo to rerun build.rs if shaders change
    println!("cargo:rerun-if-changed=shaders/");

    let mut compiler = shaderc::Compiler::new().ok_or("Failed to create shader compiler")?;
    let shader_dir = Path::new("shaders");
    let out_dir = shader_dir;

    // Compile vertex shader (Mercator)
    let vert_source = fs::read_to_string(shader_dir.join("tile.vert"))?;
    let vert_spirv = compiler.compile_into_spirv(
        &vert_source,
        shaderc::ShaderKind::Vertex,
        "tile.vert",
        "main",
        None,
    )?;
    fs::write(out_dir.join("tile.vert.spv"), vert_spirv.as_binary_u8())?;

    // Compile simple vertex shader
    let vert_simple_source = fs::read_to_string(shader_dir.join("tile_simple.vert"))?;
    let vert_simple_spirv = compiler.compile_into_spirv(
        &vert_simple_source,
        shaderc::ShaderKind::Vertex,
        "tile_simple.vert",
        "main",
        None,
    )?;
    fs::write(out_dir.join("tile_simple.vert.spv"), vert_simple_spirv.as_binary_u8())?;

    // Compile debug vertex shader
    let vert_debug_source = fs::read_to_string(shader_dir.join("tile_debug.vert"))?;
    let vert_debug_spirv = compiler.compile_into_spirv(
        &vert_debug_source,
        shaderc::ShaderKind::Vertex,
        "tile_debug.vert",
        "main",
        None,
    )?;
    fs::write(out_dir.join("tile_debug.vert.spv"), vert_debug_spirv.as_binary_u8())?;

    // Compile fragment shader
    let frag_source = fs::read_to_string(shader_dir.join("tile.frag"))?;
    let frag_spirv = compiler.compile_into_spirv(
        &frag_source,
        shaderc::ShaderKind::Fragment,
        "tile.frag",
        "main",
        None,
    )?;
    fs::write(out_dir.join("tile.frag.spv"), frag_spirv.as_binary_u8())?;

    println!("Shaders compiled successfully");
    Ok(())
}
