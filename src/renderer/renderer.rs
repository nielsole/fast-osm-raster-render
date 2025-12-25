use super::command::*;
use super::memory::*;
use super::pipeline::*;
use super::vulkan::{VulkanContext, VulkanError};
use crate::data::mmap::{MappedData, MapObjectView};
use crate::data::spatial::TileIndex;
use crate::data::types::{BoundingBox, Tile};
use crate::projection::get_bounding_box;
use ash::vk;
use gpu_allocator::vulkan::{Allocation, Allocator};
use gpu_allocator::MemoryLocation;
use image::RgbaImage;
use std::sync::{Arc, Mutex};

/// Uniform buffer object matching the shader layout
#[repr(C, align(256))]
#[derive(Copy, Clone)]
struct UniformBufferObject {
    bbox: [f32; 4],          // minLon, minLat, maxLon, maxLat
    tile_size: f32,          // 256.0
    _padding: [f32; 11],     // Padding to 64 bytes
    projection: [[f32; 4]; 4], // 4x4 matrix
}

/// Vulkan renderer for OSM tiles
pub struct VulkanRenderer {
    // Reusable resources
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,

    // Render target (framebuffer + staging buffer)
    render_target: Option<RenderTarget>,

    // Pre-allocated vertex buffer
    vertex_buffer: Option<vk::Buffer>,
    vertex_buffer_allocation: Option<Allocation>,
    vertex_buffer_capacity: usize,

    // Vulkan pipeline resources
    render_pass: vk::RenderPass,
    descriptor_set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    descriptor_pool: vk::DescriptorPool,

    // Memory manager must be dropped before context
    memory_manager: Arc<Mutex<Allocator>>,
    // Context should be dropped last
    context: VulkanContext,
}

struct RenderTarget {
    framebuffer: vk::Framebuffer,
    color_image: vk::Image,
    color_image_view: vk::ImageView,
    color_image_allocation: Allocation,
    staging_buffer: vk::Buffer,
    staging_buffer_allocation: Allocation,
}

impl VulkanRenderer {
    /// Create a new Vulkan renderer
    pub fn new(max_points: usize, shader_type: ShaderType) -> Result<Self, VulkanError> {
        // Ensure we have a minimum buffer size even with no data
        let max_points = max_points.max(1000); // Minimum 1000 points

        log::info!("Creating Vulkan renderer with {:?} shader", shader_type);

        let context = VulkanContext::new()?;

        let memory_manager = {
            // ash Instance and Device wrap raw handles and are cheap to clone
            let allocator = Allocator::new(&gpu_allocator::vulkan::AllocatorCreateDesc {
                instance: context.instance.clone(),
                device: context.device.clone(),
                physical_device: context.physical_device,
                debug_settings: Default::default(),
                buffer_device_address: false,
                allocation_sizes: Default::default(),
            })?;
            Arc::new(Mutex::new(allocator))
        };

        // Create render pass and pipeline
        let descriptor_set_layout = create_descriptor_set_layout(&context.device)?;
        let render_pass = create_render_pass(&context.device, vk::Format::R8G8B8A8_UNORM)?;
        let (pipeline, pipeline_layout) = create_graphics_pipeline(
            &context.device,
            render_pass,
            descriptor_set_layout,
            shader_type,
        )?;

        // Create descriptor pool
        let descriptor_pool = create_descriptor_pool(&context.device)?;

        // Allocate command buffer
        let command_buffer = allocate_command_buffer(&context.device, context.command_pool)?;

        // Create fence
        let fence = create_fence(&context.device, false)?;

        // Pre-allocate vertex buffer
        // Tiles can have tens of thousands of objects with complex geometry
        // Allocate a large fixed buffer (10M floats = 40MB)
        let vertex_buffer_capacity = 10_000_000; // Fixed large allocation
        let (vertex_buffer, vertex_buffer_allocation) = {
            let mut allocator = memory_manager.lock().unwrap();
            create_buffer(
                &context.device,
                &mut allocator,
                (vertex_buffer_capacity * std::mem::size_of::<f32>()) as vk::DeviceSize,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                MemoryLocation::CpuToGpu,
                "vertex_buffer",
            )?
        };

        Ok(VulkanRenderer {
            context,
            memory_manager,
            render_pass,
            descriptor_set_layout,
            pipeline_layout,
            pipeline,
            descriptor_pool,
            command_buffer,
            fence,
            render_target: None,
            vertex_buffer: Some(vertex_buffer),
            vertex_buffer_allocation: Some(vertex_buffer_allocation),
            vertex_buffer_capacity,
        })
    }

    /// Render a tile and return the image
    pub fn render_tile(
        &mut self,
        tile: &Tile,
        tile_index: &TileIndex,
        mmap_data: &MappedData,
    ) -> Result<RgbaImage, VulkanError> {
        // Get map object offsets for this tile
        let offsets = match tile_index.get(tile) {
            Some(offsets) => offsets,
            None => {
                log::warn!("No tile index data for tile {:?}", tile);
                // No data for this tile, return empty white image
                return Ok(RgbaImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgba([255, 255, 255, 255])));
            }
        };

        log::info!("Rendering tile {:?} with {} map objects", tile, offsets.len());

        // Get bounding box for tile
        let bbox = get_bounding_box(tile);
        log::info!("Tile bbox: min=({}, {}), max=({}, {})",
                   bbox.min.lon, bbox.min.lat, bbox.max.lon, bbox.max.lat);

        // Ensure render target exists
        if self.render_target.is_none() {
            self.render_target = Some(self.create_render_target()?);
        }

        // Build vertex buffer
        let vertex_count = self.build_vertex_buffer(&offsets, mmap_data, &bbox)?;

        log::info!("Built vertex buffer with {} vertices", vertex_count);

        if vertex_count == 0 {
            log::warn!("No visible vertices, returning white image");
            // No visible vertices, return white image
            return Ok(RgbaImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgba([255, 255, 255, 255])));
        }

        // Create uniform buffer
        let (uniform_buffer, uniform_allocation) = self.create_uniform_buffer(&bbox)?;

        // Create descriptor set
        let descriptor_set = self.create_descriptor_set(uniform_buffer)?;

        // Record and submit commands
        self.record_and_submit_commands(vertex_count, descriptor_set)?;

        // Read back image
        let image = self.read_framebuffer()?;

        // Cleanup
        unsafe {
            self.context.device.destroy_buffer(uniform_buffer, None);
        }
        {
            let mut allocator = self.memory_manager.lock().unwrap();
            allocator.free(uniform_allocation)?;
        }

        Ok(image)
    }

    fn create_render_target(&self) -> Result<RenderTarget, VulkanError> {
        let mut allocator = self.memory_manager.lock().unwrap();

        // Create color image
        let (color_image, color_image_allocation) = create_image(
            &self.context.device,
            &mut allocator,
            TILE_SIZE,
            TILE_SIZE,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
            MemoryLocation::GpuOnly,
            "color_image",
        )?;

        // Create image view
        let color_image_view = create_image_view(
            &self.context.device,
            color_image,
            vk::Format::R8G8B8A8_UNORM,
        )?;

        // Create framebuffer
        let attachments = [color_image_view];
        let framebuffer_info = vk::FramebufferCreateInfo::default()
            .render_pass(self.render_pass)
            .attachments(&attachments)
            .width(TILE_SIZE)
            .height(TILE_SIZE)
            .layers(1);

        let framebuffer = unsafe {
            self.context.device.create_framebuffer(&framebuffer_info, None)?
        };

        // Create staging buffer for readback
        let staging_size = (TILE_SIZE * TILE_SIZE * 4) as vk::DeviceSize;
        let (staging_buffer, staging_buffer_allocation) = create_buffer(
            &self.context.device,
            &mut allocator,
            staging_size,
            vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuToCpu,
            "staging_buffer",
        )?;

        Ok(RenderTarget {
            framebuffer,
            color_image,
            color_image_view,
            color_image_allocation,
            staging_buffer,
            staging_buffer_allocation,
        })
    }

    fn build_vertex_buffer(
        &mut self,
        offsets: &[u64],
        mmap_data: &MappedData,
        bbox: &BoundingBox,
    ) -> Result<usize, VulkanError> {
        let vertex_buffer_allocation = self.vertex_buffer_allocation.as_ref().unwrap();

        // Map vertex buffer
        let data_ptr = vertex_buffer_allocation.mapped_ptr().unwrap().as_ptr() as *mut f32;
        let mut vertex_count = 0;

        unsafe {
            let vertices = std::slice::from_raw_parts_mut(data_ptr, self.vertex_buffer_capacity);
            let mut index = 0;

            for (i, &offset) in offsets.iter().enumerate() {
                let map_object = mmap_data.read_map_object(offset);
                let obj_bbox = map_object.bounding_box();
                let points = map_object.points();

                log::debug!("Map object {}: bbox=({}, {}) to ({}, {}), {} points",
                          i, obj_bbox.min.lon, obj_bbox.min.lat,
                          obj_bbox.max.lon, obj_bbox.max.lat, points.len());

                // Check if bounding box overlaps
                if !bbox.overlaps(obj_bbox) {
                    log::debug!("  -> Skipped (no overlap)");
                    continue;
                }

                // Add line segments (pairs of points)
                if points.len() < 2 {
                    log::debug!("  -> Skipped (not enough points: {})", points.len());
                    continue;
                }

                for i in 1..points.len() {
                    if index + 4 > self.vertex_buffer_capacity {
                        log::warn!("Vertex buffer overflow, stopping");
                        break;
                    }

                    // Previous point
                    vertices[index] = points[i - 1].lon as f32;
                    vertices[index + 1] = points[i - 1].lat as f32;

                    // Current point
                    vertices[index + 2] = points[i].lon as f32;
                    vertices[index + 3] = points[i].lat as f32;

                    if vertex_count < 6 {  // Log first 3 lines only
                        log::info!("    Line {}: ({}, {}) -> ({}, {})",
                                  vertex_count / 2,
                                  points[i - 1].lon, points[i - 1].lat,
                                  points[i].lon, points[i].lat);
                    }

                    index += 4;
                    vertex_count += 2;
                }
                log::debug!("  -> Added {} line segments", points.len() - 1);
            }
        }

        Ok(vertex_count)
    }

    fn create_uniform_buffer(&self, bbox: &BoundingBox) -> Result<(vk::Buffer, Allocation), VulkanError> {
        let ubo = UniformBufferObject {
            bbox: [
                bbox.min.lon as f32,
                bbox.min.lat as f32,
                bbox.max.lon as f32,
                bbox.max.lat as f32,
            ],
            tile_size: TILE_SIZE as f32,
            _padding: [0.0; 11],
            projection: create_orthographic_projection(),
        };

        log::info!("UBO: bbox=({}, {}, {}, {}), tileSize={}",
                   ubo.bbox[0], ubo.bbox[1], ubo.bbox[2], ubo.bbox[3], ubo.tile_size);

        let mut allocator = self.memory_manager.lock().unwrap();
        let (buffer, allocation) = create_buffer(
            &self.context.device,
            &mut allocator,
            std::mem::size_of::<UniformBufferObject>() as vk::DeviceSize,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            MemoryLocation::CpuToGpu,
            "uniform_buffer",
        )?;

        // Copy data using byte-wise copy for safety
        unsafe {
            let data_ptr = allocation.mapped_ptr().unwrap().as_ptr() as *mut u8;
            let ubo_bytes = std::slice::from_raw_parts(
                &ubo as *const UniformBufferObject as *const u8,
                std::mem::size_of::<UniformBufferObject>(),
            );
            std::ptr::copy_nonoverlapping(ubo_bytes.as_ptr(), data_ptr, ubo_bytes.len());
        }

        Ok((buffer, allocation))
    }

    fn create_descriptor_set(&self, uniform_buffer: vk::Buffer) -> Result<vk::DescriptorSet, VulkanError> {
        let set_layouts = [self.descriptor_set_layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&set_layouts);

        let descriptor_sets = unsafe {
            self.context.device.allocate_descriptor_sets(&alloc_info)?
        };
        let descriptor_set = descriptor_sets[0];

        let buffer_info = vk::DescriptorBufferInfo::default()
            .buffer(uniform_buffer)
            .offset(0)
            .range(std::mem::size_of::<UniformBufferObject>() as vk::DeviceSize);

        let buffer_infos = [buffer_info];
        let descriptor_write = vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&buffer_infos);

        let descriptor_writes = [descriptor_write];
        unsafe {
            self.context.device.update_descriptor_sets(&descriptor_writes, &[]);
        }

        Ok(descriptor_set)
    }

    fn record_and_submit_commands(&mut self, vertex_count: usize, descriptor_set: vk::DescriptorSet) -> Result<(), VulkanError> {
        let render_target = self.render_target.as_ref().unwrap();

        reset_fence(&self.context.device, self.fence)?;

        unsafe {
            self.context.device.reset_command_buffer(
                self.command_buffer,
                vk::CommandBufferResetFlags::empty(),
            )?;
        }

        begin_command_buffer(&self.context.device, self.command_buffer)?;

        // Begin render pass (it will transition from UNDEFINED to COLOR_ATTACHMENT_OPTIMAL automatically)
        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [1.0, 1.0, 1.0, 1.0], // White background
            },
        }];

        let render_pass_info = vk::RenderPassBeginInfo::default()
            .render_pass(self.render_pass)
            .framebuffer(render_target.framebuffer)
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D { width: TILE_SIZE, height: TILE_SIZE },
            })
            .clear_values(&clear_values);

        unsafe {
            self.context.device.cmd_begin_render_pass(
                self.command_buffer,
                &render_pass_info,
                vk::SubpassContents::INLINE,
            );

            self.context.device.cmd_bind_pipeline(
                self.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );

            self.context.device.cmd_bind_vertex_buffers(
                self.command_buffer,
                0,
                &[self.vertex_buffer.unwrap()],
                &[0],
            );

            self.context.device.cmd_bind_descriptor_sets(
                self.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[descriptor_set],
                &[],
            );

            self.context.device.cmd_draw(self.command_buffer, vertex_count as u32, 1, 0, 0);

            self.context.device.cmd_end_render_pass(self.command_buffer);
        }

        // Copy image to staging buffer (already in TRANSFER_SRC_OPTIMAL layout from render pass)
        copy_image_to_buffer(
            &self.context.device,
            self.command_buffer,
            render_target.color_image,
            render_target.staging_buffer,
            TILE_SIZE,
            TILE_SIZE,
        );

        end_command_buffer(&self.context.device, self.command_buffer)?;

        // Submit
        submit_command_buffer(
            &self.context.device,
            self.context.queue,
            self.command_buffer,
            self.fence,
        )?;

        // Wait
        wait_for_fence(&self.context.device, self.fence, u64::MAX)?;

        Ok(())
    }

    fn read_framebuffer(&self) -> Result<RgbaImage, VulkanError> {
        let render_target = self.render_target.as_ref().unwrap();

        let staging_ptr = render_target.staging_buffer_allocation.mapped_ptr().unwrap().as_ptr();

        let image_data = unsafe {
            std::slice::from_raw_parts(
                staging_ptr as *const u8,
                (TILE_SIZE * TILE_SIZE * 4) as usize,
            )
        };

        let image = RgbaImage::from_raw(TILE_SIZE, TILE_SIZE, image_data.to_vec())
            .ok_or_else(|| VulkanError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to create image from buffer",
            )))?;

        Ok(image)
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        unsafe {
            self.context.device.device_wait_idle().ok();

            if let Some(render_target) = self.render_target.take() {
                self.context.device.destroy_framebuffer(render_target.framebuffer, None);
                self.context.device.destroy_image_view(render_target.color_image_view, None);
                self.context.device.destroy_image(render_target.color_image, None);
                self.context.device.destroy_buffer(render_target.staging_buffer, None);

                let mut allocator = self.memory_manager.lock().unwrap();
                allocator.free(render_target.color_image_allocation).ok();
                allocator.free(render_target.staging_buffer_allocation).ok();
            }

            if let Some(vertex_buffer) = self.vertex_buffer.take() {
                self.context.device.destroy_buffer(vertex_buffer, None);
                if let Some(allocation) = self.vertex_buffer_allocation.take() {
                    let mut allocator = self.memory_manager.lock().unwrap();
                    allocator.free(allocation).ok();
                }
            }

            self.context.device.destroy_fence(self.fence, None);
            self.context.device.destroy_descriptor_pool(self.descriptor_pool, None);
            self.context.device.destroy_pipeline(self.pipeline, None);
            self.context.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.context.device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            self.context.device.destroy_render_pass(self.render_pass, None);
        }
    }
}

fn create_orthographic_projection() -> [[f32; 4]; 4] {
    // Orthographic projection matching Go implementation
    // Maps 0-256 pixel space to NDC (-1 to 1)
    // NOTE: GLSL uses column-major, so we need to transpose
    let size = TILE_SIZE as f32;

    // TRANSPOSED for column-major GLSL
    [
        [2.0 / size, 0.0, 0.0, -1.0],      // Column 0
        [0.0, -2.0 / size, 0.0, 1.0],       // Column 1
        [0.0, 0.0, 1.0, 0.0],                // Column 2
        [0.0, 0.0, 0.0, 1.0],                // Column 3
    ]
}

fn create_descriptor_pool(device: &ash::Device) -> Result<vk::DescriptorPool, vk::Result> {
    let pool_size = vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(10);

    let pool_sizes = [pool_size];
    let pool_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(10)
        .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET);

    unsafe { device.create_descriptor_pool(&pool_info, None) }
}
