# As-shot WB 元数据骨架

新增到 `RawMetadata`：

- `white_balance: Option<String>`（EXIF WhiteBalance）
- `color_temperature_k: Option<u32>`（EXIF Temperature）
- `light_source: Option<String>`（EXIF LightSource）
- `exposure_bias_ev: Option<f32>`（EXIF ExposureBiasValue）

解析在 RAW 解码时完成（`kamadak-exif`，纯 Rust），不改变渲染。

## 实测

```bash
fdxmp-cli decode DSC09784.ARW
```

输出：

```text
元数据：Sony ILCE-7M4 · ISO 50 · 1/125s · f/4.5 · 75mm
白平衡：manual white balance  daylight
曝光补偿：+0.00 EV
```

说明：

- EXIF 里能拿到白平衡模式和光源；
- 这台 A7 IV 的文件没有色温 Kelvin 字段，`color_temperature_k` 为空；
- 因此不能只靠 EXIF 还原 as-shot 色温，仍需要相机 WB 系数/RAW 元数据。

## 用途

- 按 `make/model` 自动选择 DCP / CameraProfile；
- 在 UI 中显示 as-shot WB 信息；
- 后续若拿到相机 WB 系数，可直接驱动白平衡 stage。

当前只是元数据骨架，**没有改变默认白平衡行为**。
