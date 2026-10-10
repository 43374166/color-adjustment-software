//! wgpu 渲染上下文（Metal / Vulkan / DX12）。
//!
//! 阶段 1 在此基础上实现图像显示与色彩管线的 compute shader。

use crate::error::{FdxmpError, Result};
use crate::geometry::GeometryMode;
use crate::gpu_pipeline::{ColorPipeline, PipelineImage};
use crate::params::EditParams;

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
        let features = adapter.features();
        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fdxmp-device"),
                required_features: features,
                required_limits: limits,
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

/// GPU 预览会话：常驻设备 / 管线 / 源纹理。
///
/// 移动端实时预览用：源图只上传一次，参数变化时重渲染并回读 RGBA8。
pub struct PreviewSession {
    context: GpuContext,
    pipeline: ColorPipeline,
    image: PipelineImage,
    width: u32,
    height: u32,
}

impl PreviewSession {
    /// 用 RGBA8 源图创建会话（内部请求 GPU 适配器）。
    pub fn new(rgba: &[u8], width: u32, height: u32) -> Result<Self> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let context = GpuContext::new_blocking()?;
            let pipeline = ColorPipeline::new(&context.device);
            let image = pipeline.upload(&context.device, &context.queue, rgba, width, height)?;
            Ok(Self {
                context,
                pipeline,
                image,
                width,
                height,
            })
        }))
        .map_err(|_| FdxmpError::Gpu("GPU 初始化 panic（驱动能力不足？）".to_string()))?
    }

    /// 按参数渲染一帧并回读（返回 宽、高、RGBA8）。
    pub fn render(&mut self, params: &EditParams) -> Result<(u32, u32, Vec<u8>)> {
        let crop_active = params.crop.rect != [0.0, 0.0, 1.0, 1.0];
        if crop_active && !params.masks.is_empty() {
            // 裁剪 + 蒙版组合在 GPU 下坐标不一致，交给 CPU 预览保证正确性。
            return Err(FdxmpError::Gpu("裁剪与蒙版组合使用 CPU 预览".to_string()));
        }
        // 几何（旋转 / 透视 / 镜头 / 裁剪）全部在 GPU 上完成。
        self.pipeline.render_with(
            &self.context.device,
            &self.context.queue,
            &self.image,
            params,
            GeometryMode::Crop,
        );
        let bytes =
            self.pipeline
                .readback(&self.context.device, &self.context.queue, &self.image)?;
        let (width, height) = self
            .pipeline
            .target_size()
            .unwrap_or((self.width, self.height));
        Ok((width, height, bytes))
    }

    /// 后端名称（vulkan / gles / …）。
    pub fn backend(&self) -> &'static str {
        self.context.backend_name()
    }
}
