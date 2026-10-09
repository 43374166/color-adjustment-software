# fdxmp API 概览

## 1. Rust 核心（`fdxmp-core`）

```rust
use fdxmp_core::{Engine, ABI_VERSION, version};
use fdxmp_core::params::{EditParams, SliderId};
use fdxmp_core::preset::PresetFile;

// 引擎：参数 + 历史 + 缓存
let mut engine = Engine::new();
let mut params = engine.params().clone();
params.set_slider(SliderId::Exposure, 1.0);
engine.apply("曝光", params);
engine.undo();

// 参数 JSON（FFI / 持久化通用表示）
let json = engine.params_json()?;

// 预设
let preset = PresetFile::new("基础校正", "fdxmp", EditParams::default());
let bytes = preset.to_bytes()?;          // .fdxmp（ZIP）
let back = PresetFile::from_bytes(&bytes)?;

// 解码探测
let probe = fdxmp_core::decode::probe("photo.cr3")?;

// 缓存（「我的」Tab：查看大小 / 清除）
let cache = fdxmp_core::cache::CacheManager::new("cache-root");
let total = cache.total_size()?;
let freed = cache.clear_all()?;
```

### 关键类型

| 类型 | 说明 |
| --- | --- |
| `EditParams` | 全部调色参数（15 滑块 + 曲线 + HSL + 分级 + 细节 + 镜头 + 变换 + 裁剪 + 蒙版） |
| `SliderId` | 15 个调色滑块的顺序 / 标签 / 范围（与规格一致） |
| `MaskParams` / `MaskKind` | 画笔、线性 / 径向渐变、颜色范围、亮度范围、深度范围、AI 蒙版 |
| `PresetFile` / `LutAsset` | `.fdxmp` 格式 |
| `History` | 撤销 / 重做 / 跳转 / 快照 |
| `CacheManager` / `CacheKind` | 缩略图 / 预览 / AI 模型缓存 |
| `ExportSettings` | 导出格式 / 质量 / 尺寸 / 色彩空间 / 元数据 / 水印 |
| `Engine` | 门面：参数、历史、缓存 |

### feature 开关

| feature | 内容 | 默认 |
| --- | --- | --- |
| `gpu` | wgpu 上下文 | 关 |
| `raw` | rsraw（LibRaw） | 关 |
| `heif` | libheif-rs | 关 |
| `full` | 以上全部 | 关 |

## 2. C ABI（`fdxmp-ffi`）

完整函数表见 `docs/FFI.md`，头文件为 `crates/ffi/include/fdxmp.h`。

```c
void *engine = fdxmp_engine_new();
char *params = fdxmp_engine_params_json(engine);
/* ...修改 JSON... */
int32_t rc = fdxmp_engine_apply_params_json(engine, "曝光", modified);
fdxmp_string_free(params);
fdxmp_engine_free(engine);
```

## 3. 命令行（`fdxmp-cli`）

```text
fdxmp-cli version
fdxmp-cli probe <image>
fdxmp-cli preset-new <out.fdxmp> <name> [author]
fdxmp-cli preset-info <preset.fdxmp>
fdxmp-cli cache-size <dir>
fdxmp-cli cache-clear <dir>
```

