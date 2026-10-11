//! wgpu 渲染上下文（Metal / Vulkan / DX12）。
//!
//! - [`GpuContext`]：设备 / 队列 / 实例 / 适配器（多会话共享同一个设备）；
//! - [`PreviewSession`]：移动端实时预览会话（源图上传一次，参数变化重渲染）；
//!   - 回读模式：`render` 渲完把显示纹理读回 RGBA8；
//!   - 零拷贝模式（iOS / macOS）：`attach_layer` 挂上 CAMetalLayer，
//!     `present` 直接把显示纹理拷到绘制帧上屏，不经过 CPU；
//!   - `histogram`：GPU 直方图（4×256，读回 4KB）。

use crate::error::{FdxmpError, Result};
use crate::geometry::GeometryMode;
use crate::gpu_pipeline::{ColorPipeline, HistogramPass, PipelineImage, HISTOGRAM_BINS};
use crate::params::EditParams;

/// GPU 设备与队列（含实例 / 适配器：创建显示表面要复用同一个实例）。
pub struct GpuContext {
    /// wgpu 实例（创建显示表面时复用）。
    pub instance: wgpu::Instance,
    /// 选中的适配器（创建显示表面时复用）。
    pub adapter: wgpu::Adapter,
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
            instance,
            adapter,
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

/// GPU 预览会话：常驻管线 / 源纹理；渲染 → 回读或直接上屏。
pub struct PreviewSession {
    /// 仅零拷贝上屏（apple）用得到；其他平台保留句柄不影响行为。
    #[allow(dead_code)]
    instance: wgpu::Instance,
    #[allow(dead_code)]
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    backend: &'static str,
    pipeline: ColorPipeline,
    image: PipelineImage,
    width: u32,
    height: u32,
    histogram: HistogramPass,
    #[cfg(target_vendor = "apple")]
    presenter: Option<SurfacePresenter>,
}

impl PreviewSession {
    /// 用 RGBA8 源图创建会话（内部请求 GPU 适配器）。
    pub fn new(rgba: &[u8], width: u32, height: u32) -> Result<Self> {
        let context = GpuContext::new_blocking()?;
        Self::new_with_context(&context, rgba, width, height)
    }

    /// 用共享的 [`GpuContext`] 创建会话（多次创建不再重复初始化设备）。
    pub fn new_with_context(
        context: &GpuContext,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let pipeline = ColorPipeline::new(&context.device);
            let image = pipeline.upload(&context.device, &context.queue, rgba, width, height)?;
            let histogram = HistogramPass::new(&context.device);
            Ok(Self {
                instance: context.instance.clone(),
                adapter: context.adapter.clone(),
                device: context.device.clone(),
                queue: context.queue.clone(),
                backend: context.backend_name(),
                pipeline,
                image,
                width,
                height,
                histogram,
                #[cfg(target_vendor = "apple")]
                presenter: None,
            })
        }));
        match result {
            Ok(inner) => inner,
            Err(_) => Err(FdxmpError::Gpu(
                "GPU 初始化 panic（驱动能力不足？）".to_string(),
            )),
        }
    }

    /// 按参数渲染一帧并回读（返回 宽、高、RGBA8）。
    pub fn render(&mut self, params: &EditParams) -> Result<(u32, u32, Vec<u8>)> {
        self.render_pipeline(params)?;
        let bytes = self
            .pipeline
            .readback(&self.device, &self.queue, &self.image)?;
        let (width, height) = self
            .pipeline
            .target_size()
            .unwrap_or((self.width, self.height));
        Ok((width, height, bytes))
    }

    /// 渲染核心（几何 / 镜头 / 裁剪全部在 GPU 上完成）。
    fn render_pipeline(&mut self, params: &EditParams) -> Result<()> {
        let crop_active = params.crop.rect != [0.0, 0.0, 1.0, 1.0];
        if crop_active && !params.masks.is_empty() {
            // 裁剪 + 蒙版组合在 GPU 下坐标不一致，交给 CPU 预览保证正确性。
            return Err(FdxmpError::Gpu("裁剪与蒙版组合使用 CPU 预览".to_string()));
        }
        self.pipeline.render_with(
            &self.device,
            &self.queue,
            &self.image,
            params,
            GeometryMode::Crop,
        );
        Ok(())
    }

    /// GPU 直方图：4×256 桶（R / G / B / 亮度），基于最近一帧显示纹理。
    pub fn histogram(&mut self) -> Result<[u32; HISTOGRAM_BINS]> {
        let (width, height) = self
            .pipeline
            .target_size()
            .ok_or_else(|| FdxmpError::Gpu("尚未渲染，无法统计直方图".to_string()))?;
        let view = self
            .pipeline
            .output_view()
            .ok_or_else(|| FdxmpError::Gpu("尚未渲染，无法统计直方图".to_string()))?;
        self.histogram
            .compute(&self.device, &self.queue, view, width, height)
    }

    /// 后端名称（vulkan / metal / …）。
    pub fn backend(&self) -> &'static str {
        self.backend
    }

    /// 挂载 CAMetalLayer（零拷贝上屏；iOS / macOS）。
    ///
    /// # Safety
    ///
    /// `layer` 必须是存活的 `CAMetalLayer` 指针，且在会话使用期间一直有效。
    #[cfg(target_vendor = "apple")]
    pub fn attach_layer(&mut self, layer: *mut std::ffi::c_void) -> Result<()> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let presenter = unsafe { SurfacePresenter::new(&self.instance, &self.adapter, layer) }?;
            self.presenter = Some(presenter);
            Ok(())
        }));
        match result {
            Ok(inner) => inner,
            Err(_) => Err(FdxmpError::Gpu(
                "挂载显示图层 panic（驱动能力不足？）".to_string(),
            )),
        }
    }

    /// 渲染一帧并直接上屏（不读回像素；返回输出尺寸）。
    #[cfg(target_vendor = "apple")]
    pub fn present(&mut self, params: &EditParams) -> Result<(u32, u32)> {
        self.render_pipeline(params)?;
        let (width, height) = self
            .pipeline
            .target_size()
            .unwrap_or((self.width, self.height));
        let texture = self
            .pipeline
            .display_texture()
            .ok_or_else(|| FdxmpError::Gpu("尚未渲染，无法上屏".to_string()))?;
        let presenter = self
            .presenter
            .as_mut()
            .ok_or_else(|| FdxmpError::Gpu("未挂载显示图层".to_string()))?;
        presenter.present(&self.device, &self.queue, texture, width, height)?;
        Ok((width, height))
    }
}

/// CAMetalLayer 显示表面（零拷贝上屏）。
#[cfg(target_vendor = "apple")]
pub struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    configured: bool,
}

#[cfg(target_vendor = "apple")]
impl SurfacePresenter {
    /// # Safety
    ///
    /// `layer` 必须是存活的 `CAMetalLayer` 指针。
    unsafe fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        layer: *mut std::ffi::c_void,
    ) -> Result<Self> {
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(|err| FdxmpError::Gpu(format!("创建显示表面失败：{err}")))?;
        let capabilities = surface.get_capabilities(adapter);
        let Some(format) = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| *format == wgpu::TextureFormat::Rgba8Unorm)
            .or_else(|| capabilities.formats.first().copied())
        else {
            return Err(FdxmpError::Gpu("显示表面没有可用格式".to_string()));
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::COPY_DST,
            format,
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
        };
        Ok(Self {
            surface,
            config,
            configured: false,
        })
    }

    fn configure(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if !self.configured || self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(device, &self.config);
            self.configured = true;
        }
    }

    fn present(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.configure(device, width, height);
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                // 尺寸 / 状态变化：重新配置后重试一次。
                self.configured = false;
                self.configure(device, width, height);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame) => frame,
                    wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                    other => {
                        return Err(FdxmpError::Gpu(format!("获取显示帧失败：{other:?}")));
                    }
                }
            }
            other => return Err(FdxmpError::Gpu(format!("获取显示帧失败：{other:?}"))),
        };
        let copy_width = width.min(frame.texture.width());
        let copy_height = height.min(frame.texture.height());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-present"),
        });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &frame.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: copy_width,
                height: copy_height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        queue.present(frame);
        Ok(())
    }
}
