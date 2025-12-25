use ash::vk;
use std::ffi::CStr;
use std::os::raw::c_char;

/// Vulkan context for headless rendering
pub struct VulkanContext {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue_family_index: u32,
    pub queue: vk::Queue,
    pub command_pool: vk::CommandPool,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
}

impl VulkanContext {
    /// Create a new Vulkan context for headless rendering
    pub fn new() -> Result<Self, VulkanError> {
        let entry = unsafe { ash::Entry::load()? };

        // Create Vulkan instance
        let instance = create_instance(&entry)?;

        // Find physical device
        let (physical_device, queue_family_index) = find_suitable_physical_device(&instance)?;

        // Get memory properties
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(physical_device) };

        // Create logical device
        let (device, queue) = create_device(&instance, physical_device, queue_family_index)?;

        // Create command pool
        let command_pool = create_command_pool(&device, queue_family_index)?;

        Ok(VulkanContext {
            entry,
            instance,
            physical_device,
            device,
            queue_family_index,
            queue,
            command_pool,
            memory_properties,
        })
    }

    /// Find a memory type that matches the requirements
    pub fn find_memory_type(
        &self,
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        for i in 0..self.memory_properties.memory_type_count {
            if (type_filter & (1 << i)) != 0
                && self.memory_properties.memory_types[i as usize]
                    .property_flags
                    .contains(properties)
            {
                return Some(i);
            }
        }
        None
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// Create Vulkan instance
fn create_instance(entry: &ash::Entry) -> Result<ash::Instance, VulkanError> {
    let app_name = std::ffi::CString::new("Rust OSM Renderer").unwrap();
    let engine_name = std::ffi::CString::new("No Engine").unwrap();

    let app_info = vk::ApplicationInfo::default()
        .application_name(&app_name)
        .application_version(vk::make_api_version(0, 1, 0, 0))
        .engine_name(&engine_name)
        .engine_version(vk::make_api_version(0, 1, 0, 0))
        .api_version(vk::API_VERSION_1_2);

    // Enable validation layers in debug mode
    #[cfg(debug_assertions)]
    let layer_names = vec![std::ffi::CString::new("VK_LAYER_KHRONOS_validation").unwrap()];
    #[cfg(debug_assertions)]
    let layer_names_raw: Vec<*const c_char> = layer_names
        .iter()
        .map(|name| name.as_ptr())
        .collect();

    #[cfg(not(debug_assertions))]
    let layer_names_raw: Vec<*const c_char> = vec![];

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_layer_names(&layer_names_raw);

    let instance = unsafe { entry.create_instance(&create_info, None)? };

    Ok(instance)
}

/// Find a suitable physical device (GPU) for rendering
fn find_suitable_physical_device(
    instance: &ash::Instance,
) -> Result<(vk::PhysicalDevice, u32), VulkanError> {
    let physical_devices = unsafe { instance.enumerate_physical_devices()? };

    if physical_devices.is_empty() {
        return Err(VulkanError::NoPhysicalDevice);
    }

    for physical_device in physical_devices {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) };

        log::info!(
            "Found physical device: {:?} (type: {:?})",
            device_name,
            properties.device_type
        );

        // Find graphics queue family
        let queue_families =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

        for (index, queue_family) in queue_families.iter().enumerate() {
            if queue_family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                log::info!(
                    "Selected device: {:?}, queue family: {}",
                    device_name,
                    index
                );
                return Ok((physical_device, index as u32));
            }
        }
    }

    Err(VulkanError::NoSuitableQueueFamily)
}

/// Create logical device and queue
fn create_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
) -> Result<(ash::Device, vk::Queue), VulkanError> {
    let queue_priorities = [1.0f32];

    let queue_create_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family_index)
        .queue_priorities(&queue_priorities);

    let queue_create_infos = [queue_create_info];
    let device_create_info =
        vk::DeviceCreateInfo::default().queue_create_infos(&queue_create_infos);

    let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };

    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    Ok((device, queue))
}

/// Create command pool
fn create_command_pool(
    device: &ash::Device,
    queue_family_index: u32,
) -> Result<vk::CommandPool, VulkanError> {
    let pool_create_info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(queue_family_index)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

    let command_pool = unsafe { device.create_command_pool(&pool_create_info, None)? };

    Ok(command_pool)
}

/// Vulkan-specific errors
#[derive(Debug, thiserror::Error)]
pub enum VulkanError {
    #[error("Failed to load Vulkan library")]
    LoadError(#[from] ash::LoadingError),

    #[error("Vulkan error: {0}")]
    VkError(#[from] vk::Result),

    #[error("No suitable physical device found")]
    NoPhysicalDevice,

    #[error("No suitable queue family found")]
    NoSuitableQueueFamily,

    #[error("Failed to find suitable memory type")]
    NoSuitableMemoryType,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("GPU allocator error: {0}")]
    AllocationError(#[from] gpu_allocator::AllocationError),
}

// Placeholder for complete rendering functionality
// This will be implemented in the full render_tile() function
