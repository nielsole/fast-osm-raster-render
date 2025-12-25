use ash::vk;
use std::fs::File;
use std::io::Read;

pub const TILE_SIZE: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderType {
    Mercator,
    Simple,
    Debug,
}

/// Create a graphics pipeline for rendering lines
pub fn create_graphics_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    descriptor_set_layout: vk::DescriptorSetLayout,
    shader_type: ShaderType,
) -> Result<(vk::Pipeline, vk::PipelineLayout), vk::Result> {
    // Load shader modules
    let vert_path = match shader_type {
        ShaderType::Mercator => "shaders/tile.vert.spv",
        ShaderType::Simple => "shaders/tile_simple.vert.spv",
        ShaderType::Debug => "shaders/tile_debug.vert.spv",
    };
    let vert_shader_module = create_shader_module(device, vert_path)?;
    let frag_shader_module = create_shader_module(device, "shaders/tile.frag.spv")?;

    let entry_point = std::ffi::CString::new("main").unwrap();

    let vert_stage_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::VERTEX)
        .module(vert_shader_module)
        .name(&entry_point);

    let frag_stage_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::FRAGMENT)
        .module(frag_shader_module)
        .name(&entry_point);

    let shader_stages = [vert_stage_info, frag_stage_info];

    // Vertex input: 2 floats (lon, lat)
    let vertex_binding_descriptions = [vk::VertexInputBindingDescription::default()
        .binding(0)
        .stride(8) // 2 * f32
        .input_rate(vk::VertexInputRate::VERTEX)];

    let vertex_attribute_descriptions = [vk::VertexInputAttributeDescription::default()
        .binding(0)
        .location(0)
        .format(vk::Format::R32G32_SFLOAT)
        .offset(0)];

    let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&vertex_binding_descriptions)
        .vertex_attribute_descriptions(&vertex_attribute_descriptions);

    // Input assembly: line list
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::LINE_LIST)
        .primitive_restart_enable(false);

    // Viewport and scissor
    let viewports = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: TILE_SIZE as f32,
        height: TILE_SIZE as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    }];

    let scissors = [vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent: vk::Extent2D {
            width: TILE_SIZE,
            height: TILE_SIZE,
        },
    }];

    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewports(&viewports)
        .scissors(&scissors);

    // Rasterization
    let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(false)
        .rasterizer_discard_enable(false)
        .polygon_mode(vk::PolygonMode::FILL)
        .line_width(1.0)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .depth_bias_enable(false);

    // Multisampling (disabled)
    let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
        .sample_shading_enable(false)
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);

    // Color blending
    let color_blend_attachment = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA)
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
        .alpha_blend_op(vk::BlendOp::ADD);

    let color_blend_attachments = [color_blend_attachment];

    let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
        .logic_op_enable(false)
        .attachments(&color_blend_attachments);

    // Pipeline layout (descriptor sets)
    let set_layouts = [descriptor_set_layout];

    let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts);

    let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

    // Create pipeline
    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&shader_stages)
        .vertex_input_state(&vertex_input_info)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterizer)
        .multisample_state(&multisampling)
        .color_blend_state(&color_blending)
        .layout(pipeline_layout)
        .render_pass(render_pass)
        .subpass(0);

    let pipelines = unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
    }
    .map_err(|(_, e)| e)?;

    let pipeline = pipelines[0];

    // Clean up shader modules
    unsafe {
        device.destroy_shader_module(vert_shader_module, None);
        device.destroy_shader_module(frag_shader_module, None);
    }

    Ok((pipeline, pipeline_layout))
}

/// Create render pass
pub fn create_render_pass(
    device: &ash::Device,
    format: vk::Format,
) -> Result<vk::RenderPass, vk::Result> {
    let color_attachment = vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL);

    let color_attachment_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

    let color_attachments = [color_attachment_ref];

    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachments);

    let dependency = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);

    let attachments = [color_attachment];
    let subpasses = [subpass];
    let dependencies = [dependency];

    let render_pass_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    unsafe { device.create_render_pass(&render_pass_info, None) }
}

/// Create descriptor set layout for uniforms
pub fn create_descriptor_set_layout(
    device: &ash::Device,
) -> Result<vk::DescriptorSetLayout, vk::Result> {
    let ubo_layout_binding = vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::VERTEX);

    let bindings = [ubo_layout_binding];

    let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);

    unsafe { device.create_descriptor_set_layout(&layout_info, None) }
}

/// Load shader SPIR-V bytecode
fn create_shader_module(device: &ash::Device, path: &str) -> Result<vk::ShaderModule, vk::Result> {
    let mut file = File::open(path).expect("Failed to open shader file");
    let mut code = Vec::new();
    file.read_to_end(&mut code)
        .expect("Failed to read shader file");

    // Ensure proper alignment
    let code = ash::util::read_spv(&mut std::io::Cursor::new(&code))
        .expect("Failed to read SPIR-V");

    let create_info = vk::ShaderModuleCreateInfo::default().code(&code);

    unsafe { device.create_shader_module(&create_info, None) }
}
