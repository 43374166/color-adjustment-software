# 示例预设

本目录存放 `.fdxmp` 示例预设（JSON + 可选内嵌 LUT 的 ZIP 包）。

生成方式：

```powershell
cargo run -p fdxmp-cli -- preset-new presets/base.fdxmp "基础校正" fdxmp
cargo run -p fdxmp-cli -- preset-info presets/base.fdxmp
```

格式定义见 `crates/core/src/preset.rs` 与 `docs/SPEC.md`。

