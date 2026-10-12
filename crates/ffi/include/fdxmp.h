/* fdxmp C ABI —— 四端（Windows / Android / HarmonyOS / iOS）共用接口。
 *
 * 约定：
 *   - 返回的 char* 必须用 fdxmp_string_free 释放；
 *   - 返回的 (uint8_t*, size_t) 必须用 fdxmp_bytes_free 释放；
 *   - fdxmp_version() 返回静态字符串，禁止释放；
 *   - 失败时返回 NULL / -1 / UINT64_MAX，并可通过 fdxmp_last_error_message() 获取原因；
 *   - 错误信息为 thread-local，需在同一个线程读取。
 */

#ifndef FDXMP_H
#define FDXMP_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FDXMP_ABI_VERSION 1
#define FDXMP_OK 0
#define FDXMP_ERR (-1)
#define FDXMP_U64_ERR UINT64_MAX

/* ---- 版本与错误 ---- */

uint32_t fdxmp_abi_version(void);
const char *fdxmp_version(void);
char *fdxmp_last_error_message(void);
void fdxmp_string_free(char *ptr);
void fdxmp_bytes_free(uint8_t *ptr, size_t len);

/* ---- 引擎 ---- */

void *fdxmp_engine_new(void);
void *fdxmp_engine_new_with_cache_root(const char *cache_root);
void fdxmp_engine_free(void *engine);
char *fdxmp_engine_params_json(const void *engine);
int32_t fdxmp_engine_apply_params_json(void *engine, const char *label, const char *json);
int32_t fdxmp_engine_undo(void *engine);
int32_t fdxmp_engine_redo(void *engine);
size_t fdxmp_engine_history_len(const void *engine);
size_t fdxmp_engine_history_index(const void *engine);

/* ---- 预设（.fdxmp） ---- */

char *fdxmp_preset_params_json_from_bytes(const uint8_t *bytes, size_t len);
uint8_t *fdxmp_preset_bytes_from_params_json(const char *json,
                                             const char *name,
                                             const char *author,
                                             size_t *out_len);
/* 预设元数据 + 参数（name / author / description / params）；失败返回 NULL。 */
char *fdxmp_preset_json_from_bytes(const uint8_t *bytes, size_t len);
/* 从 Lightroom XMP 文本导入预设（name / author / params）；失败返回 NULL。 */
char *fdxmp_preset_json_from_xmp(const char *xmp);

/* ---- 解码探测 ---- */

char *fdxmp_probe_json(const char *path);

/* ---- RAW 解码（LibRaw / rsraw，需构建时启用 raw feature）----
 *
 * 从内存字节（如 iOS 从 PhotoKit 取到的原始 DNG / RAW 数据）解码相机 RAW；
 * full_size = 0 半尺寸解调（预览）/ 非 0 全尺寸解调（导出）；
 * 输出按 max_edge 等比缩小（不放大）。返回格式与 fdxmp_render_preview_rgba 相同
 * （8 字节大端宽高 + RGBA8 像素），必须用 fdxmp_bytes_free(ptr, len) 释放。
 */
uint8_t *fdxmp_decode_raw_rgba8(const uint8_t *bytes,
                                size_t len,
                                uint32_t max_edge,
                                uint32_t full_size,
                                uint32_t *out_width,
                                uint32_t *out_height,
                                size_t *out_len);

/* ---- 预览渲染（CPU，移动端 / 无 GPU 环境）----
 *
 * 返回缓冲区前 8 字节为大端 width / height，其后为行优先 RGBA8 像素；
 * 必须用 fdxmp_bytes_free(ptr, len) 释放。失败返回 NULL。
 */
uint8_t *fdxmp_render_preview_rgba(const char *path,
                                   const char *params_json,
                                   uint32_t max_edge,
                                   uint32_t *out_width,
                                   uint32_t *out_height,
                                   size_t *out_len);

/* ---- 内存图像渲染（系统解码，如 iOS PhotoKit）----
 *
 * 输入：行优先 RGBA8 像素（至少 width × height × 4 字节）+ EditParams JSON；
 * 输出格式与 fdxmp_render_preview_rgba 相同（8 字节大端宽高 + 像素），
 * 必须用 fdxmp_bytes_free(ptr, len) 释放。失败返回 NULL。
 */
uint8_t *fdxmp_render_memory_rgba(const uint8_t *rgba,
                                  uint32_t width,
                                  uint32_t height,
                                  const char *params_json,
                                  uint32_t *out_width,
                                  uint32_t *out_height,
                                  size_t *out_len);

/* ---- 区域渲染（放大 1:1 看细节）----
 *
 * 输入：区域像素（区域 = 可见矩形向外扩余量后夹到整帧内）+ 各矩形坐标；
 * 输出：可见矩形的像素（格式同 fdxmp_render_memory_rgba，8 字节大端宽高 + RGBA8），
 * 暗角 / 颗粒与蒙版按整帧坐标计算，与整帧渲染逐像素一致。
 * 要求几何 / 镜头为单位变换且蒙版不依赖图像内容，否则返回 NULL（调用方回退整帧渲染）。
 */
uint8_t *fdxmp_render_region_rgba(const uint8_t *rgba,
                                  uint32_t region_width,
                                  uint32_t region_height,
                                  const char *params_json,
                                  uint32_t region_x,
                                  uint32_t region_y,
                                  uint32_t visible_x,
                                  uint32_t visible_y,
                                  uint32_t visible_width,
                                  uint32_t visible_height,
                                  uint32_t full_width,
                                  uint32_t full_height,
                                  uint32_t *out_width,
                                  uint32_t *out_height,
                                  size_t *out_len);

/* ---- 蒙版叠加层（移动端蒙版编辑器 / 蒙版列表缩略图）----
 *
 * 输入：EditParams JSON + 蒙版 id + 目标宽高（预览画布或缩略图尺寸）；
 * 输出：8 字节大端宽高 + 预乘 RGBA8 像素（红色半透明覆盖），
 * 必须用 fdxmp_bytes_free(ptr, len) 释放。失败返回 NULL。
 * 支持几何型蒙版（画笔 / 线性 / 径向）与智能蒙版（AI / 平台提交的栅格，
 * 见 fdxmp_mask_raster_submit / fdxmp_mask_polygons_submit）；隐藏的蒙版
 * 返回全透明叠加。
 */
uint8_t *fdxmp_mask_overlay_rgba(const char *params_json,
                                 const char *mask_id,
                                 uint32_t width,
                                 uint32_t height,
                                 uint32_t *out_width,
                                 uint32_t *out_height,
                                 size_t *out_len);

/* ---- 智能蒙版（v1.4.0）----
 *
 * 平台提供者（iOS Vision / CoreML、桌面 ONNX）把像素级蒙版交给核心统一缓存、
 * 渲染、叠加与导出：栅格按蒙版参数缓存（与分辨率无关，按请求自动重采样），
 * 参数变化 / 切换图片后自然失效（也可主动 fdxmp_mask_raster_clear）。
 */

/* 提交权重栅格：weights 为行优先 f32（0..1），长度必须等于 width × height；
 * 单边 ≤ 4096、总像素 ≤ 16777216。成功返回 FDXMP_OK。 */
int32_t fdxmp_mask_raster_submit(const char *params_json,
                                 const char *mask_id,
                                 uint32_t width,
                                 uint32_t height,
                                 const float *weights,
                                 size_t len);

/* 蒙版栅格是否已生成：1 = 就绪，0 = 未生成，负值 = 参数错误。 */
int32_t fdxmp_mask_raster_ready(const char *params_json, const char *mask_id);

/* 清空智能蒙版栅格缓存（切换图片 / 释放内存时调用）。 */
int32_t fdxmp_mask_raster_clear(void);

/* 提交多边形蒙版（五官关键点）：polygons_json 形如
 * {"include": [[[x,y],...]], "exclude": [[[x,y],...]]}，归一化坐标 0..1
 * （原点左上角）；核心负责光栅化、羽化（feather_px，0..=128）与缓存。 */
int32_t fdxmp_mask_polygons_submit(const char *params_json,
                                   const char *mask_id,
                                   uint32_t width,
                                   uint32_t height,
                                   const char *polygons_json,
                                   float feather_px);

/* 智能蒙版模型清单与状态（JSON；含 SHA-256 校验状态、许可证、来源）。
 * 返回的字符串用 fdxmp_string_free 释放；失败返回 NULL。 */
char *fdxmp_smartmask_models_json(void);

/* 用随包 ONNX 模型生成智能蒙版（双阶段流水线），写入共享栅格缓存。
 * 返回：0 成功；2 模型/运行库不可用或该类型不支持（调用方回退平台实现，
 * 如 iOS Vision）；-1 推理失败（原因见 fdxmp_last_error_message）。 */
#define FDXMP_SMARTMASK_FALLBACK 2
int32_t fdxmp_smartmask_generate(const char *params_json,
                                 const char *mask_id,
                                 const uint8_t *rgba,
                                 uint32_t width,
                                 uint32_t height);

/* 随包 ONNX 模型是否可用：1 = 就绪，0 = 不可用。 */
int32_t fdxmp_smartmask_runtime_ready(void);

/* ---- AI 修图（v1.5.0）----
 *
 * 一键修图：识别（五官 478 点 + 皮肤 / 人像蒙版）→ 应用（去瑕疵 / 低中高频磨皮 /
 * 统一肤色 / 脸部塑形 / 身体塑形）。修图只改像素，不参与调色管线。
 */

/* 识别：返回 JSON {"faces", "landmarks", "skin", "person", "warnings"}；
 * 用 fdxmp_string_free 释放；失败返回 NULL。 */
char *fdxmp_retouch_analyze(const uint8_t *rgba, uint32_t width, uint32_t height);

/* 应用修图：params_json 为 RetouchParams JSON（见核心 retouch::RetouchParams）；
 * 返回 8 字节大端宽高 + RGBA8，用 fdxmp_bytes_free(ptr, len) 释放；失败返回 NULL。 */
uint8_t *fdxmp_retouch_apply(const char *params_json,
                             const uint8_t *rgba,
                             uint32_t width,
                             uint32_t height,
                             uint32_t *out_width,
                             uint32_t *out_height,
                             size_t *out_len);

/* ---- GPU 预览会话（Metal / DX12 / Vulkan；iOS 以 `--features gpu` 构建启用）----
 *
 * 移动端实时预览：源图只上传一次，参数变化时重渲染并回读 RGBA8。
 * 创建失败（无 GPU / 驱动不足 / 未启用 feature）返回 NULL，调用方回退 CPU 预览；
 * 「裁剪 + 蒙版」组合在 GPU 下坐标不一致，render 也返回 NULL
 * （错误信息见 fdxmp_last_error_message），同样由调用方回退 CPU。
 * 句柄用 fdxmp_gpu_preview_destroy 释放；返回缓冲区用 fdxmp_bytes_free 释放。
 */

/* 创建会话；`rgba` 为行优先 RGBA8（至少 width × height × 4 字节）。失败返回 NULL。 */
void *fdxmp_gpu_preview_create(const uint8_t *rgba, uint32_t width, uint32_t height);

/* 用参数 JSON 渲染一帧；返回格式同 fdxmp_render_memory_rgba（8 字节大端宽高 + RGBA8）。 */
uint8_t *fdxmp_gpu_preview_render(void *handle,
                                  const char *params_json,
                                  uint32_t *out_width,
                                  uint32_t *out_height,
                                  size_t *out_len);

/* 后端名称（char* 用 fdxmp_string_free 释放）；失败返回 NULL。 */
char *fdxmp_gpu_preview_backend(void *handle);

/* 零拷贝上屏（iOS / macOS）：把 CAMetalLayer 挂到会话上；成功 0，失败 -1。
 * layer 必须是存活的 CAMetalLayer 指针，且在会话使用期间一直有效。 */
int32_t fdxmp_gpu_preview_attach_layer(void *handle, void *layer);

/* 渲染一帧并直接上屏（不读回像素）；输出尺寸写入 out_width / out_height。
 * 成功 0，失败 -1（调用方回退回读路径）。 */
int32_t fdxmp_gpu_preview_present(void *handle,
                                  const char *params_json,
                                  uint32_t *out_width,
                                  uint32_t *out_height);

/* GPU 直方图：4×256 个小端 u32（R / G / B / 亮度，共 4096 字节）；
 * 缓冲区用 fdxmp_bytes_free 释放；失败返回 NULL。 */
uint8_t *fdxmp_gpu_preview_histogram(void *handle, size_t *out_len);

/* 释放会话（handle 可为 NULL）。 */
void fdxmp_gpu_preview_destroy(void *handle);

/* ---- 缓存 ---- */

uint64_t fdxmp_cache_total_size(const char *cache_root);
uint64_t fdxmp_cache_clear(const char *cache_root);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* FDXMP_H */
