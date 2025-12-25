use ash::vk;

/// Allocate a command buffer from a command pool
pub fn allocate_command_buffer(
    device: &ash::Device,
    command_pool: vk::CommandPool,
) -> Result<vk::CommandBuffer, vk::Result> {
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);

    let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };

    Ok(command_buffers[0])
}

/// Begin a command buffer for one-time submit
pub fn begin_command_buffer(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
) -> Result<(), vk::Result> {
    let begin_info = vk::CommandBufferBeginInfo::default()
        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

    unsafe { device.begin_command_buffer(command_buffer, &begin_info)? };

    Ok(())
}

/// End command buffer recording
pub fn end_command_buffer(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
) -> Result<(), vk::Result> {
    unsafe { device.end_command_buffer(command_buffer)? };

    Ok(())
}

/// Submit command buffer to queue with a fence
pub fn submit_command_buffer(
    device: &ash::Device,
    queue: vk::Queue,
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,
) -> Result<(), vk::Result> {
    let command_buffers = [command_buffer];

    let submit_info = vk::SubmitInfo::default().command_buffers(&command_buffers);

    unsafe {
        device.queue_submit(queue, &[submit_info], fence)?;
    }

    Ok(())
}

/// Wait for a fence to be signaled
pub fn wait_for_fence(
    device: &ash::Device,
    fence: vk::Fence,
    timeout: u64,
) -> Result<(), vk::Result> {
    unsafe { device.wait_for_fences(&[fence], true, timeout)? };

    Ok(())
}

/// Reset a fence
pub fn reset_fence(device: &ash::Device, fence: vk::Fence) -> Result<(), vk::Result> {
    unsafe { device.reset_fences(&[fence])? };

    Ok(())
}

/// Create a fence
pub fn create_fence(
    device: &ash::Device,
    signaled: bool,
) -> Result<vk::Fence, vk::Result> {
    let flags = if signaled {
        vk::FenceCreateFlags::SIGNALED
    } else {
        vk::FenceCreateFlags::empty()
    };

    let fence_info = vk::FenceCreateInfo::default().flags(flags);

    unsafe { device.create_fence(&fence_info, None) }
}

/// Transition image layout using a pipeline barrier
pub fn transition_image_layout(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
    let (src_access_mask, dst_access_mask, src_stage, dst_stage) =
        match (old_layout, new_layout) {
            (vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL) => (
                vk::AccessFlags::empty(),
                vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
            ),
            (vk::ImageLayout::UNDEFINED, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL) => (
                vk::AccessFlags::empty(),
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ),
            (vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL, vk::ImageLayout::TRANSFER_SRC_OPTIMAL) => (
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::AccessFlags::TRANSFER_READ,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::TRANSFER,
            ),
            _ => {
                log::warn!(
                    "Unsupported layout transition: {:?} -> {:?}",
                    old_layout,
                    new_layout
                );
                return;
            }
        };

    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })
        .src_access_mask(src_access_mask)
        .dst_access_mask(dst_access_mask);

    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    }
}

/// Copy image to buffer
pub fn copy_image_to_buffer(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    buffer: vk::Buffer,
    width: u32,
    height: u32,
) {
    let region = vk::BufferImageCopy::default()
        .buffer_offset(0)
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        })
        .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
        .image_extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        });

    unsafe {
        device.cmd_copy_image_to_buffer(
            command_buffer,
            image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            buffer,
            &[region],
        );
    }
}
