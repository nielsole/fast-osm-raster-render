use super::vulkan::VulkanContext;
use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use gpu_allocator::MemoryLocation;
use std::sync::{Arc, Mutex};

/// Manages Vulkan memory allocations
pub struct MemoryManager {
    allocator: Arc<Mutex<Allocator>>,
}

impl MemoryManager {
    /// Create a new memory manager
    pub fn new(context: &VulkanContext) -> Result<Self, gpu_allocator::AllocationError> {
        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: context.instance.clone(),
            device: context.device.clone(),
            physical_device: context.physical_device,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })?;

        Ok(MemoryManager {
            allocator: Arc::new(Mutex::new(allocator)),
        })
    }

    /// Get a reference to the allocator
    pub fn allocator(&self) -> Arc<Mutex<Allocator>> {
        self.allocator.clone()
    }
}

/// Helper function to create a buffer with gpu-allocator
pub fn create_buffer(
    device: &ash::Device,
    allocator: &mut Allocator,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    location: MemoryLocation,
    name: &str,
) -> Result<(vk::Buffer, gpu_allocator::vulkan::Allocation), gpu_allocator::AllocationError> {
    let buffer_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);

    let buffer = unsafe { device.create_buffer(&buffer_info, None) }
        .map_err(|e| gpu_allocator::AllocationError::Internal(e.to_string()))?;

    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

    let allocation = allocator.allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
        name,
        requirements,
        location,
        linear: true,
        allocation_scheme: gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
    })?;

    unsafe {
        device
            .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
            .map_err(|e| gpu_allocator::AllocationError::Internal(e.to_string()))?;
    }

    Ok((buffer, allocation))
}

/// Helper function to create an image with gpu-allocator
pub fn create_image(
    device: &ash::Device,
    allocator: &mut Allocator,
    width: u32,
    height: u32,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    location: MemoryLocation,
    name: &str,
) -> Result<(vk::Image, gpu_allocator::vulkan::Allocation), gpu_allocator::AllocationError> {
    let image_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .format(format)
        .tiling(vk::ImageTiling::OPTIMAL)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .samples(vk::SampleCountFlags::TYPE_1);

    let image = unsafe { device.create_image(&image_info, None) }
        .map_err(|e| gpu_allocator::AllocationError::Internal(e.to_string()))?;

    let requirements = unsafe { device.get_image_memory_requirements(image) };

    let allocation = allocator.allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
        name,
        requirements,
        location,
        linear: false,
        allocation_scheme: gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
    })?;

    unsafe {
        device
            .bind_image_memory(image, allocation.memory(), allocation.offset())
            .map_err(|e| gpu_allocator::AllocationError::Internal(e.to_string()))?;
    }

    Ok((image, allocation))
}

/// Helper function to create an image view
pub fn create_image_view(
    device: &ash::Device,
    image: vk::Image,
    format: vk::Format,
) -> Result<vk::ImageView, vk::Result> {
    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });

    unsafe { device.create_image_view(&view_info, None) }
}
