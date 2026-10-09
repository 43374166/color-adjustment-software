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

/* ---- 缓存 ---- */

uint64_t fdxmp_cache_total_size(const char *cache_root);
uint64_t fdxmp_cache_clear(const char *cache_root);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* FDXMP_H */

