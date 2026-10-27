//! 直方图统计（阶段 1：CPU 抽样；阶段 2 改为 GPU compute 统计）。

/// 直方图分箱数量。
pub const BINS: usize = 256;

/// 单次统计的最大采样像素数（大图按步长抽样，保证 UI 刷新速度）。
pub const MAX_SAMPLES: usize = 2_000_000;

/// 波形图列数（横向分辨率；显示时按控件宽度缩放）。
pub const WAVE_COLUMNS: usize = 256;
/// 波形图纵向桶数（比直方图粗，省内存：3×256×128×4 ≈ 384 KB）。
pub const WAVE_BINS: usize = 128;

/// RGB + 亮度直方图，外加 RGB 波形图（每列 × 每桶的样本数）。
// 注意：serde 只实现了 ≤32 元素的数组，直方图不直接参与序列化
//（如需持久化，导出为 Vec<u32> 或 base64）。
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    /// 红通道。
    pub red: [u32; BINS],
    /// 绿通道。
    pub green: [u32; BINS],
    /// 蓝通道。
    pub blue: [u32; BINS],
    /// 亮度（Rec.709 权重）。
    pub luma: [u32; BINS],
    /// 波形图：`[通道][列][桶]`，通道顺序 R/G/B。
    pub waveform: Vec<u32>,
    /// 实际采样像素数。
    pub samples: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            red: [0; BINS],
            green: [0; BINS],
            blue: [0; BINS],
            luma: [0; BINS],
            waveform: vec![0; 3 * WAVE_COLUMNS * WAVE_BINS],
            samples: 0,
        }
    }
}

impl Histogram {
    /// 从图像统计（自动按步长抽样）。
    pub fn from_image(image: &image::DynamicImage) -> Self {
        let rgba = image.to_rgba8();
        Self::from_rgba8(rgba.as_raw(), rgba.width() as usize, rgba.height() as usize)
    }

    /// 从 RGBA8 字节统计。
    pub fn from_rgba8(bytes: &[u8], width: usize, height: usize) -> Self {
        let mut histogram = Self::default();
        let total = width.saturating_mul(height);
        if total == 0 {
            return histogram;
        }
        let stride = (total / MAX_SAMPLES).max(1);

        let mut index = 0usize;
        while index < total {
            let offset = index * 4;
            if offset + 3 >= bytes.len() {
                break;
            }
            let (r, g, b) = (
                bytes[offset] as usize,
                bytes[offset + 1] as usize,
                bytes[offset + 2] as usize,
            );
            histogram.red[r] += 1;
            histogram.green[g] += 1;
            histogram.blue[b] += 1;
            let luma = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
            histogram.luma[(luma as usize).min(BINS - 1)] += 1;

            // 波形图：按列（x 方向）统计各通道的分布。
            let column = (index % width) * WAVE_COLUMNS / width;
            let base = column * WAVE_BINS;
            let plane = WAVE_COLUMNS * WAVE_BINS;
            histogram.waveform[base + r * WAVE_BINS / BINS] += 1;
            histogram.waveform[plane + base + g * WAVE_BINS / BINS] += 1;
            histogram.waveform[2 * plane + base + b * WAVE_BINS / BINS] += 1;

            histogram.samples += 1;
            index += stride;
        }
        histogram
    }

    /// 波形图单列数据（`channel`：0=红 / 1=绿 / 2=蓝）。
    pub fn wave_column(&self, channel: usize, column: usize) -> &[u32] {
        let base = (channel.min(2) * WAVE_COLUMNS + column.min(WAVE_COLUMNS - 1)) * WAVE_BINS;
        &self.waveform[base..base + WAVE_BINS]
    }

    /// 所有通道中的最大分箱值（用于绘制归一化）。
    pub fn peak(&self) -> u32 {
        let mut peak = 1;
        for bin in 0..BINS {
            peak = peak
                .max(self.red[bin])
                .max(self.green[bin])
                .max(self.blue[bin])
                .max(self.luma[bin]);
        }
        peak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_black_lands_in_first_bin() {
        let bytes = vec![0u8; 4 * 4 * 4];
        let histogram = Histogram::from_rgba8(&bytes, 4, 4);
        assert_eq!(histogram.samples, 16);
        assert_eq!(histogram.red[0], 16);
        assert_eq!(histogram.luma[0], 16);
        assert_eq!(histogram.red[255], 0);
    }

    #[test]
    fn constant_white_lands_in_last_bin() {
        let bytes = vec![255u8; 2 * 2 * 4];
        let histogram = Histogram::from_rgba8(&bytes, 2, 2);
        assert_eq!(histogram.samples, 4);
        assert_eq!(histogram.green[255], 4);
        assert_eq!(histogram.luma[255], 4);
    }

    #[test]
    fn large_images_are_sampled_without_overflow() {
        let bytes = vec![128u8; 4 * 4 * 4];
        let histogram = Histogram::from_rgba8(&bytes, 100_000, 100_000);
        assert!(histogram.samples > 0);
        assert!(histogram.samples <= MAX_SAMPLES as u64 + 1);
        assert_eq!(histogram.peak(), histogram.samples as u32);
    }
}
