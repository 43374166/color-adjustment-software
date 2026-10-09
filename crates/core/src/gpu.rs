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
            let mut pipeline = ColorPipeline::new(&context.device);
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
        // 几何趟在部分移动驱动上会把画面缩小，这里只用 GPU 做调色，
        // 裁剪在 CPU 侧做（简单拷贝，代价很小）。
        self.pipeline.render_with(
            &self.context.device,
            &self.context.queue,
            &self.image,
            params,
            GeometryMode::Full,
        );
        let bytes = self.pipeline.readback(
            &self.context.device,
            &self.context.queue,
            &self.image,
        )?;
        let (full_width, full_height) = self
            .pipeline
            .target_size()
            .unwrap_or((self.width, self.height));
        if bytes.len() < (full_width * full_height * 4) as usize {
            return Err(FdxmpError::Gpu("GPU 回读数据不完整".to_string()));
        }
        crop_rgba(bytes, full_width, full_height, params.crop.rect)
    }

    /// 后端名称（vulkan / gles / …）。
    pub fn backend(&self) -> &'static str {
        self.context.backend_name()
    }
}

/// 按归一化矩形裁剪 RGBA8 数据。
fn crop_rgba(
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    rect: [f32; 4],
) -> Result<(u32, u32, Vec<u8>)> {
    let x = rect[0].clamp(0.0, 0.99);
    let y = rect[1].clamp(0.0, 0.99);
    let w = rect[2].clamp(0.01, 1.0 - x);
    let h = rect[3].clamp(0.01, 1.0 - y);
    let left = (x * width as f32).round() as u32;
    let top = (y * height as f32).round() as u32;
    let crop_width = ((w * width as f32).round() as u32).clamp(1, width - left.min(width - 1));
    let crop_height =
        ((h * height as f32).round() as u32).clamp(1, height - top.min(height - 1));
    if left == 0 && top == 0 && crop_width == width && crop_height == height {
        return Ok((width, height, bytes));
    }
    let mut out = Vec::with_capacity((crop_width * crop_height * 4) as usize);
    for row in 0..crop_height {
        let start = (((top + row) * width + left) * 4) as usize;
        let end = start + (crop_width * 4) as usize;
        out.extend_from_slice(&bytes[start..end]);
    }
    Ok((crop_width, crop_height, out))
}
