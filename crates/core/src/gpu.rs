//! wgpu 渲染上下文（Metal / Vulkan / DX12）。
//!
//! 阶段 1 在此基础上实现图像显示与色彩管线的 compute shader。

use crate::error::{FdxmpError, Result};

/// GPU 设备与队列。
pub struct GpuContext {
    /// 设备信息（名称、后端、类型）。
    pub adapter_info: wgpu::AdapterInfo,
    /// wgpu 设备。
    pub device: wgpu::Device,
    /// wgpu 队列。
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// 同步创建（内部使用 tokio 当前线程运行时）。
    pub fn new_blocking() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| FdxmpError::Gpu(err.to_string()))?;
        runtime.block_on(Self::new())
    }

    /// 异步创建：请求适配器与设备。
    pub async fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|err| FdxmpError::Gpu(format!("请求 GPU 适配器失败：{err}")))?;

        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fdxmp-device"),
                ..Default::default()
            })
            .await
            .map_err(|err| FdxmpError::Gpu(format!("请求 GPU 设备失败：{err}")))?;

        Ok(Self {
            adapter_info,
            device,
            queue,
        })
    }

    /// 后端名称（dx12 / vulkan / metal / webgpu）。
    pub fn backend_name(&self) -> &'static str {
        match self.adapter_info.backend {
            wgpu::Backend::Dx12 => "dx12",
            wgpu::Backend::Vulkan => "vulkan",
            wgpu::Backend::Metal => "metal",
            wgpu::Backend::Gl => "gl",
            wgpu::Backend::BrowserWebGpu => "webgpu",
            _ => "unknown",
        }
    }
}
