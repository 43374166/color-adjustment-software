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

/* ---- 解码探测 ---- */

char *fdxmp_probe_json(const char *path);

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

/* ---- 蒙版叠加层（移动端蒙版编辑器 / 蒙版列表缩略图）----
 *
 * 输入：EditParams JSON + 蒙版 id + 目标宽高（预览画布或缩略图尺寸）；
 * 输出：8 字节大端宽高 + 预乘 RGBA8 像素（红色半透明覆盖），
 * 必须用 fdxmp_bytes_free(ptr, len) 释放。失败返回 NULL。
 * 只支持几何型蒙版（画笔 / 线性 / 径向）；隐藏的蒙版返回全透明叠加。
 */
uint8_t *fdxmp_mask_overlay_rgba(const char *params_json,
                                 const char *mask_id,
                                 uint32_t width,
                                 uint32_t height,
                                 uint32_t *out_width,
                                 uint32_t *out_height,
                                 size_t *out_len);

/* ---- 缓存 ---- */

uint64_t fdxmp_cache_total_size(const char *cache_root);
uint64_t fdxmp_cache_clear(const char *cache_root);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* FDXMP_H */
