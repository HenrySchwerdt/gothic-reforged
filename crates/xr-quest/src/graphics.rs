//! Raw Vulkan bootstrap through `XR_KHR_vulkan_enable2` and wgpu-hal adoption.
//!
//! The OpenXR runtime has to create the `VkInstance` and `VkDevice` (it knows which physical device drives the
//! HMD and which extensions the compositor needs), so wgpu's own initialisation cannot be used. The flow follows
//! the OpenXR spec and is adapted from the structure of `bevy_mod_openxr` (MIT/Apache-2.0), updated for wgpu 30.
#![allow(unsafe_code)]

use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_void};

use anyhow::{Context, Result, anyhow, bail};
use ash::vk::{self, Handle};
use openxr as xr;
use wgpu::hal::api::Vulkan;
use wgpu::hal::vulkan as hal_vk;

/// Vulkan API version requested from the runtime. Quest (Android) exposes Vulkan 1.1+ to applications.
pub const VK_TARGET: xr::Version = xr::Version::new(1, 1, 0);
const VK_TARGET_ASH: u32 = vk::make_api_version(0, 1, 1, 0);

/// Everything Bevy's `RenderCreation::manual` needs.
pub struct WgpuGraphics {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    pub adapter: wgpu::Adapter,
    pub instance: wgpu::Instance,
}

/// Vulkan handles for `xrCreateSession`.
pub struct VkSessionInfo {
    pub info: xr::vulkan::SessionCreateInfo,
}

#[cfg(target_os = "android")]
fn android_sdk_version() -> u32 {
    android_system_properties::AndroidSystemProperties::new()
        .get("ro.build.version.sdk")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
#[cfg(not(target_os = "android"))]
fn android_sdk_version() -> u32 {
    0
}

/// Creates the Vulkan instance/device through OpenXR and wraps them in wgpu.
pub fn init_graphics(
    instance: &xr::Instance,
    system: xr::SystemId,
    app_name: &str,
) -> Result<(WgpuGraphics, VkSessionInfo)> {
    let reqs = instance
        .graphics_requirements::<xr::Vulkan>(system)
        .context("xrGetVulkanGraphicsRequirements2KHR")?;
    log::info!(
        "OpenXR Vulkan requirements: min {} max {}",
        reqs.min_api_version_supported,
        reqs.max_api_version_supported
    );
    if VK_TARGET < reqs.min_api_version_supported
        || VK_TARGET.major() > reqs.max_api_version_supported.major()
    {
        bail!("OpenXR runtime does not support Vulkan {VK_TARGET}");
    }

    let vk_entry = unsafe { ash::Entry::load() }.context("load libvulkan")?;
    let flags = wgpu::InstanceFlags::default().with_env();
    let instance_exts =
        <Vulkan as wgpu::hal::Api>::Instance::desired_extensions(&vk_entry, VK_TARGET_ASH, flags)
            .map_err(|e| anyhow!("desired_extensions: {e:?}"))?;
    let mut device_exts: Vec<&'static CStr> = Vec::new();

    let ext_ptrs: Vec<_> = instance_exts.iter().map(|s| s.as_ptr()).collect();
    let app_name_c = CString::new(app_name)?;
    let vk_app_info = vk::ApplicationInfo::default()
        .application_name(&app_name_c)
        .application_version(1)
        .engine_name(c"bevy")
        .engine_version(20)
        .api_version(VK_TARGET_ASH);
    let get_proc = unsafe {
        #[allow(clippy::missing_transmute_annotations)]
        std::mem::transmute::<_, xr::sys::platform::VkGetInstanceProcAddr>(
            vk_entry.static_fn().get_instance_proc_addr,
        )
    };

    let vk_instance_raw = unsafe {
        instance
            .create_vulkan_instance(
                system,
                get_proc,
                &vk::InstanceCreateInfo::default()
                    .application_info(&vk_app_info)
                    .enabled_extension_names(&ext_ptrs) as *const _ as *const _,
            )
            .context("xrCreateVulkanInstanceKHR")?
            .map_err(|e| anyhow!("vkCreateInstance via OpenXR failed: {e}"))?
    };
    let vk_instance = unsafe {
        ash::Instance::load(
            vk_entry.static_fn(),
            vk::Instance::from_raw(vk_instance_raw as _),
        )
    };
    let vk_phys = vk::PhysicalDevice::from_raw(unsafe {
        instance
            .vulkan_graphics_device(system, vk_instance.handle().as_raw() as _)
            .context("xrGetVulkanGraphicsDevice2KHR")? as _
    });
    let props = unsafe { vk_instance.get_physical_device_properties(vk_phys) };
    log::info!(
        "Vulkan device: {:?} api {} driver {:#x}",
        props.device_name_as_c_str().unwrap_or(c"?"),
        props.api_version,
        props.driver_version
    );

    let has_nv_optimus = false;
    let wgpu_vk_instance = unsafe {
        <Vulkan as wgpu::hal::Api>::Instance::from_raw(
            vk_entry.clone(),
            vk_instance.clone(),
            props.api_version,
            android_sdk_version(),
            None,
            instance_exts,
            flags,
            wgpu::MemoryBudgetThresholds::default(),
            has_nv_optimus,
            None,
        )
        .map_err(|e| anyhow!("wgpu-hal Instance::from_raw: {e:?}"))?
    };
    let exposed = wgpu_vk_instance
        .expose_adapter(vk_phys)
        .ok_or_else(|| anyhow!("wgpu could not expose the OpenXR physical device"))?;
    let wgpu_features = exposed.features;
    log::info!("wgpu features: {wgpu_features:?}");

    let mut enabled: BTreeSet<&'static CStr> = exposed
        .adapter
        .required_device_extensions(wgpu_features)
        .into_iter()
        .collect();
    enabled.extend(device_exts.drain(..));
    let enabled: Vec<&'static CStr> = enabled.into_iter().collect();

    let queue_family = 0u32;
    let mut phd_features = exposed
        .adapter
        .physical_device_features(&enabled, wgpu_features);
    let prios = [1.0f32];
    let family_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family)
        .queue_priorities(&prios);
    let family_infos = [family_info];
    let ext_names: Vec<_> = enabled.iter().map(|e| e.as_ptr()).collect();
    let create_info = phd_features
        .add_to_device_create(vk::DeviceCreateInfo::default().queue_create_infos(&family_infos))
        .enabled_extension_names(&ext_names);

    let vk_device_raw = unsafe {
        instance
            .create_vulkan_device(
                system,
                get_proc,
                vk_phys.as_raw() as _,
                &create_info as *const _ as *const _,
            )
            .context("xrCreateVulkanDeviceKHR")?
            .map_err(|e| anyhow!("vkCreateDevice via OpenXR failed: {e}"))?
    };
    let vk_device = unsafe {
        ash::Device::load(
            vk_instance.fp_v1_0(),
            vk::Device::from_raw(vk_device_raw as _),
        )
    };
    let vk_device_ptr = vk_device.handle().as_raw() as *const c_void;

    let open_device = unsafe {
        exposed.adapter.device_from_raw(
            vk_device,
            None,
            &enabled,
            wgpu_features,
            &wgpu::Limits::defaults(),
            &wgpu::MemoryHints::Performance,
            queue_family,
            0,
        )
    }
    .map_err(|e| anyhow!("device_from_raw: {e:?}"))?;

    let wgpu_instance = unsafe { wgpu::Instance::from_hal::<Vulkan>(wgpu_vk_instance) };
    let wgpu_adapter = unsafe { wgpu_instance.create_adapter_from_hal(exposed) };
    let limits = wgpu_adapter.limits();
    log::info!(
        "wgpu limits: max_texture_2d={} max_bind_groups={} max_storage_buffers={}",
        limits.max_texture_dimension_2d,
        limits.max_bind_groups,
        limits.max_storage_buffers_per_shader_stage
    );
    let (device, queue) = unsafe {
        wgpu_adapter.create_device_from_hal(
            open_device,
            &wgpu::DeviceDescriptor {
                label: Some("quest-device"),
                required_features: wgpu_features,
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::enabled(),
            },
        )
    }
    .context("create_device_from_hal")?;

    let info = xr::vulkan::SessionCreateInfo {
        instance: vk_instance.handle().as_raw() as *const c_void,
        physical_device: vk_phys.as_raw() as *const c_void,
        device: vk_device_ptr,
        queue_family_index: queue_family,
        queue_index: 0,
    };
    let adapter_info = wgpu_adapter.get_info();
    Ok((
        WgpuGraphics {
            device,
            queue,
            adapter_info,
            adapter: wgpu_adapter,
            instance: wgpu_instance,
        },
        VkSessionInfo { info },
    ))
}

/// Wraps an OpenXR swapchain `VkImage` as a wgpu texture. The image stays owned by the runtime.
pub fn wrap_swapchain_image(
    device: &wgpu::Device,
    image: u64,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    label: &str,
) -> Result<wgpu::Texture> {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let vk_image = vk::Image::from_raw(image);
    let hal_tex = unsafe {
        let hal_dev = device
            .as_hal::<Vulkan>()
            .ok_or_else(|| anyhow!("wgpu device is not Vulkan"))?;
        hal_dev.texture_from_raw(
            vk_image,
            &wgpu::hal::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUses::COLOR_TARGET | wgpu::TextureUses::RESOURCE,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: vec![],
            },
            // Some(..) makes wgpu-hal skip vkDestroyImage: the runtime owns the image.
            Some(Box::new(|| {})),
            hal_vk::TextureMemory::External,
        )
    };
    let tex = unsafe {
        device.create_texture_from_hal::<Vulkan>(
            hal_tex,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::TextureUses::UNINITIALIZED,
        )
    };
    Ok(tex)
}

pub fn vk_format_to_wgpu(raw: u32) -> Option<wgpu::TextureFormat> {
    match vk::Format::from_raw(raw as i32) {
        vk::Format::R8G8B8A8_SRGB => Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        vk::Format::B8G8R8A8_SRGB => Some(wgpu::TextureFormat::Bgra8UnormSrgb),
        vk::Format::R8G8B8A8_UNORM => Some(wgpu::TextureFormat::Rgba8Unorm),
        vk::Format::B8G8R8A8_UNORM => Some(wgpu::TextureFormat::Bgra8Unorm),
        _ => None,
    }
}
