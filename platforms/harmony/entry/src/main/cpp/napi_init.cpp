// fdxmp HarmonyOS N-API 桥接层。
//
// ArkTS 通过 `import fdxmp from 'libfdxmp_ohos_bridge.so'` 调用这里的导出函数；
// 真正的调色 / 曲线烘焙在同目录的 `libfdxmp_ffi.so`（Rust，C ABI），构建时链接进来。
//
// 只暴露 ArkTS 需要的四个入口：版本、渲染、均衡器曲线、最后一条错误。

#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

#include "napi/native_api.h"

extern "C" {
int32_t fdxmp_abi_version(void);
const char *fdxmp_version(void);
char *fdxmp_last_error_message(void);
void fdxmp_string_free(char *ptr);
void fdxmp_bytes_free(uint8_t *ptr, size_t len);

// 返回 8 字节大端头（宽、高）+ RGBA 像素；失败返回 NULL。
uint8_t *fdxmp_render_memory_rgba(const uint8_t *rgba, uint32_t width, uint32_t height,
                                  const char *params_json, uint32_t *out_width,
                                  uint32_t *out_height, size_t *out_len);

// 分块渲染整帧（原图分辨率、内存受控），返回格式同上。
uint8_t *fdxmp_render_memory_tiled_rgba(const uint8_t *rgba, uint32_t width, uint32_t height,
                                        const char *params_json, uint32_t strip_pixels,
                                        uint32_t *out_width, uint32_t *out_height,
                                        size_t *out_len);

// 8bit sRGB → OkLCh（3 个 f32：L / C / 色相弧度）。
int32_t fdxmp_color_oklch_from_srgb8(uint8_t r, uint8_t g, uint8_t b, float *out);

// 512 点曲线；channel：0 色相 / 1 饱和度 / 2 亮度。
int32_t fdxmp_equalizer_curve(uint32_t channel, const char *params_json, float *out);
}

namespace {

std::string LastError() {
    char *message = fdxmp_last_error_message();
    if (message == nullptr) {
        return std::string();
    }
    std::string text(message);
    fdxmp_string_free(message);
    return text;
}

napi_value MakeString(napi_env env, const std::string &text) {
    napi_value result = nullptr;
    napi_create_string_utf8(env, text.c_str(), text.size(), &result);
    return result;
}

napi_value Throw(napi_env env, const std::string &message) {
    napi_throw_error(env, nullptr, message.c_str());
    return nullptr;
}

// 读取 ArkTS 传进来的字符串参数（失败返回空串）。
std::string ReadString(napi_env env, napi_value value) {
    size_t length = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
        return std::string();
    }
    std::string text(length, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, value, text.data(), length + 1, &copied) != napi_ok) {
        return std::string();
    }
    text.resize(copied);
    return text;
}

napi_value Version(napi_env env, napi_callback_info) {
    return MakeString(env, fdxmp_version());
}

napi_value AbiVersion(napi_env env, napi_callback_info) {
    napi_value result = nullptr;
    napi_create_int32(env, fdxmp_abi_version(), &result);
    return result;
}

napi_value LastErrorValue(napi_env env, napi_callback_info) {
    return MakeString(env, LastError());
}

// renderMemoryRgba(rgba: ArrayBuffer, width: number, height: number, paramsJson: string)
//   -> { width: number, height: number, data: ArrayBuffer }
napi_value RenderMemoryRgba(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
    if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 4) {
        return Throw(env, "renderMemoryRgba 需要 4 个参数");
    }

    void *pixels = nullptr;
    size_t byte_length = 0;
    if (napi_get_arraybuffer_info(env, argv[0], &pixels, &byte_length) != napi_ok) {
        return Throw(env, "第 1 个参数必须是 ArrayBuffer");
    }
    uint32_t width = 0;
    uint32_t height = 0;
    if (napi_get_value_uint32(env, argv[1], &width) != napi_ok ||
        napi_get_value_uint32(env, argv[2], &height) != napi_ok) {
        return Throw(env, "宽高必须是整数");
    }
    const std::string params = ReadString(env, argv[3]);
    if (params.empty()) {
        return Throw(env, "参数 JSON 为空");
    }

    uint32_t out_width = 0;
    uint32_t out_height = 0;
    size_t out_len = 0;
    uint8_t *payload = fdxmp_render_memory_rgba(static_cast<const uint8_t *>(pixels), width, height,
                                                params.c_str(), &out_width, &out_height, &out_len);
    if (payload == nullptr) {
        return Throw(env, LastError());
    }
    // 头部 8 字节：宽高各 4 字节大端。
    if (out_len < 8) {
        fdxmp_bytes_free(payload, out_len);
        return Throw(env, "渲染结果长度异常");
    }
    uint32_t head_width = (uint32_t(payload[0]) << 24) | (uint32_t(payload[1]) << 16) |
                          (uint32_t(payload[2]) << 8) | uint32_t(payload[3]);
    uint32_t head_height = (uint32_t(payload[4]) << 24) | (uint32_t(payload[5]) << 16) |
                           (uint32_t(payload[6]) << 8) | uint32_t(payload[7]);
    const size_t data_len = out_len - 8;

    napi_value object = nullptr;
    napi_create_object(env, &object);

    napi_value buffer = nullptr;
    void *buffer_data = nullptr;
    if (napi_create_arraybuffer(env, data_len, &buffer_data, &buffer) != napi_ok) {
        fdxmp_bytes_free(payload, out_len);
        return Throw(env, "分配输出缓冲区失败");
    }
    std::memcpy(buffer_data, payload + 8, data_len);
    fdxmp_bytes_free(payload, out_len);

    napi_value js_width = nullptr;
    napi_value js_height = nullptr;
    napi_create_uint32(env, head_width != 0 ? head_width : out_width, &js_width);
    napi_create_uint32(env, head_height != 0 ? head_height : out_height, &js_height);
    napi_set_named_property(env, object, "width", js_width);
    napi_set_named_property(env, object, "height", js_height);
    napi_set_named_property(env, object, "data", buffer);
    return object;
}

// renderTiledRgba(rgba, width, height, paramsJson, stripPixels)
//   -> { width, height, data }（分块渲染整帧：原图分辨率、内存受控）
napi_value RenderTiledRgba(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
    if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 5) {
        return Throw(env, "renderTiledRgba 需要 5 个参数");
    }

    void *pixels = nullptr;
    size_t byte_length = 0;
    if (napi_get_arraybuffer_info(env, argv[0], &pixels, &byte_length) != napi_ok) {
        return Throw(env, "第 1 个参数必须是 ArrayBuffer");
    }
    uint32_t width = 0;
    uint32_t height = 0;
    if (napi_get_value_uint32(env, argv[1], &width) != napi_ok ||
        napi_get_value_uint32(env, argv[2], &height) != napi_ok) {
        return Throw(env, "宽高必须是整数");
    }
    const std::string params = ReadString(env, argv[3]);
    if (params.empty()) {
        return Throw(env, "参数 JSON 为空");
    }
    uint32_t strip_pixels = 4000000;
    if (napi_get_value_uint32(env, argv[4], &strip_pixels) != napi_ok) {
        return Throw(env, "stripPixels 必须是整数");
    }

    uint32_t out_width = 0;
    uint32_t out_height = 0;
    size_t out_len = 0;
    uint8_t *payload = fdxmp_render_memory_tiled_rgba(
        static_cast<const uint8_t *>(pixels), width, height, params.c_str(), strip_pixels,
        &out_width, &out_height, &out_len);
    if (payload == nullptr) {
        return Throw(env, LastError());
    }
    if (out_len < 8) {
        fdxmp_bytes_free(payload, out_len);
        return Throw(env, "渲染结果长度异常");
    }
    uint32_t head_width = (uint32_t(payload[0]) << 24) | (uint32_t(payload[1]) << 16) |
                          (uint32_t(payload[2]) << 8) | uint32_t(payload[3]);
    uint32_t head_height = (uint32_t(payload[4]) << 24) | (uint32_t(payload[5]) << 16) |
                           (uint32_t(payload[6]) << 8) | uint32_t(payload[7]);
    const size_t data_len = out_len - 8;

    napi_value object = nullptr;
    napi_create_object(env, &object);

    napi_value buffer = nullptr;
    void *buffer_data = nullptr;
    if (napi_create_arraybuffer(env, data_len, &buffer_data, &buffer) != napi_ok) {
        fdxmp_bytes_free(payload, out_len);
        return Throw(env, "分配输出缓冲区失败");
    }
    std::memcpy(buffer_data, payload + 8, data_len);
    fdxmp_bytes_free(payload, out_len);

    napi_value js_width = nullptr;
    napi_value js_height = nullptr;
    napi_create_uint32(env, head_width != 0 ? head_width : out_width, &js_width);
    napi_create_uint32(env, head_height != 0 ? head_height : out_height, &js_height);
    napi_set_named_property(env, object, "width", js_width);
    napi_set_named_property(env, object, "height", js_height);
    napi_set_named_property(env, object, "data", buffer);
    return object;
}

// equalizerCurve(channel: number, paramsJson: string) -> ArrayBuffer（512 个 f32）
napi_value EqualizerCurve(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value argv[2] = {nullptr, nullptr};
    if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) {
        return Throw(env, "equalizerCurve 需要 2 个参数");
    }
    uint32_t channel = 0;
    if (napi_get_value_uint32(env, argv[0], &channel) != napi_ok) {
        return Throw(env, "channel 必须是整数");
    }
    const std::string params = ReadString(env, argv[1]);
    if (params.empty()) {
        return Throw(env, "参数 JSON 为空");
    }
    float lut[512] = {0.0f};
    if (fdxmp_equalizer_curve(channel, params.c_str(), lut) != 0) {
        return Throw(env, LastError());
    }
    napi_value buffer = nullptr;
    void *buffer_data = nullptr;
    if (napi_create_arraybuffer(env, sizeof(lut), &buffer_data, &buffer) != napi_ok) {
        return Throw(env, "分配曲线缓冲区失败");
    }
    std::memcpy(buffer_data, lut, sizeof(lut));
    return buffer;
}

// oklch(r, g, b) -> ArrayBuffer（3 个 f32：L / C / 色相弧度）
napi_value Oklch(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value argv[3] = {nullptr, nullptr, nullptr};
    if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) {
        return Throw(env, "oklch 需要 3 个参数");
    }
    uint32_t r = 0;
    uint32_t g = 0;
    uint32_t b = 0;
    if (napi_get_value_uint32(env, argv[0], &r) != napi_ok ||
        napi_get_value_uint32(env, argv[1], &g) != napi_ok ||
        napi_get_value_uint32(env, argv[2], &b) != napi_ok) {
        return Throw(env, "rgb 必须是整数");
    }
    float lch[3] = {0.0f, 0.0f, 0.0f};
    if (fdxmp_color_oklch_from_srgb8(static_cast<uint8_t>(r & 0xFF), static_cast<uint8_t>(g & 0xFF),
                                     static_cast<uint8_t>(b & 0xFF), lch) != 0) {
        return Throw(env, LastError());
    }
    napi_value buffer = nullptr;
    void *buffer_data = nullptr;
    if (napi_create_arraybuffer(env, sizeof(lch), &buffer_data, &buffer) != napi_ok) {
        return Throw(env, "分配缓冲区失败");
    }
    std::memcpy(buffer_data, lch, sizeof(lch));
    return buffer;
}

napi_value Init(napi_env env, napi_value exports) {
    napi_property_descriptor desc[] = {
        {"version", nullptr, Version, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"abiVersion", nullptr, AbiVersion, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"lastError", nullptr, LastErrorValue, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"renderTiledRgba", nullptr, RenderTiledRgba, nullptr, nullptr, nullptr, napi_default,
        nullptr},
    {"renderMemoryRgba", nullptr, RenderMemoryRgba, nullptr, nullptr, nullptr, napi_default,
         nullptr},
        {"equalizerCurve", nullptr, EqualizerCurve, nullptr, nullptr, nullptr, napi_default,
         nullptr},
        {"oklch", nullptr, Oklch, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    napi_define_properties(env, exports, sizeof(desc) / sizeof(desc[0]), desc);
    return exports;
}

napi_module g_module = {
    1,
    0,
    nullptr,
    Init,
    "fdxmp_ohos_bridge",
    nullptr,
    {0},
};

}  // namespace

extern "C" __attribute__((constructor)) void RegisterFdxmpModule(void) {
    napi_module_register(&g_module);
}
