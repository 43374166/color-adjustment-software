//! 颜色均衡器需要的空间滤波原语。
//!
//! - [`gaussian_blur`]：可分离高斯（边界复制），作为“局部均值”参与导向滤波；
//! - [`scharr_gradient`]：3×3 Scharr 梯度，用来测饱和度突变，抑制亮度修正的光晕；
//! - [`guided_filter_2ch`]：以 2 通道色度为导向图、对 2 通道修正量做导向滤波
//!   （He et al. 的 fast guided filter 的 2 通道版），先降采样、再双线性升采样。
//!
//! 所有缓冲区都是紧凑交织的 `f32`（`channels` 个分量/像素），与项目其余部分一致。
//! 行与行之间相互独立，因此按行分给多个 `std::thread`（wasm 下自动退化为单线程）。
//! 实现是独立的，不含 darktable 源码。

/// 引导滤波降采样倍率上限。
const MAX_SCALING: usize = 4;

/// 少于这么多元素就不值得开线程。
const PARALLEL_THRESHOLD: usize = 32 * 1024;

#[cfg(target_arch = "wasm32")]
fn thread_count() -> usize {
    1
}

#[cfg(not(target_arch = "wasm32"))]
fn thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(16)
}

/// 按行并行遍历一个输出缓冲区。
///
/// `f(y, row)` 处理第 `y` 行；`row` 的长度恒为 `row_len`。
pub(crate) fn par_rows_mut<F>(out: &mut [f32], row_len: usize, rows: usize, f: F)
where
    F: Fn(usize, &mut [f32]) + Sync,
{
    if row_len == 0 || rows == 0 || out.len() < rows * row_len {
        return;
    }
    let threads = thread_count().min(rows);
    if threads <= 1 || out.len() < PARALLEL_THRESHOLD {
        for (y, row) in out.chunks_mut(row_len).take(rows).enumerate() {
            f(y, row);
        }
        return;
    }

    let band = rows.div_ceil(threads);
    let mut rest = &mut out[..rows * row_len];
    std::thread::scope(|scope| {
        let mut y0 = 0;
        while y0 < rows {
            let take = band.min(rows - y0);
            let (chunk, tail) = rest.split_at_mut(take * row_len);
            rest = tail;
            let f = &f;
            scope.spawn(move || {
                for (i, row) in chunk.chunks_mut(row_len).enumerate() {
                    f(y0 + i, row);
                }
            });
            y0 += take;
        }
    });
}

/// 按行并行遍历两个输出缓冲区（两边的行宽可以不同）。
pub(crate) fn par_rows_mut2<F>(
    out_a: &mut [f32],
    row_len_a: usize,
    out_b: &mut [f32],
    row_len_b: usize,
    rows: usize,
    f: F,
) where
    F: Fn(usize, &mut [f32], &mut [f32]) + Sync,
{
    if row_len_a == 0 || row_len_b == 0 || rows == 0 {
        return;
    }
    if out_a.len() < rows * row_len_a || out_b.len() < rows * row_len_b {
        return;
    }
    let threads = thread_count().min(rows);
    if threads <= 1 || out_a.len() + out_b.len() < PARALLEL_THRESHOLD {
        let mut a = &mut out_a[..rows * row_len_a];
        let mut b = &mut out_b[..rows * row_len_b];
        for y in 0..rows {
            let (row_a, tail_a) = a.split_at_mut(row_len_a);
            let (row_b, tail_b) = b.split_at_mut(row_len_b);
            a = tail_a;
            b = tail_b;
            f(y, row_a, row_b);
        }
        return;
    }

    let band = rows.div_ceil(threads);
    let mut rest_a = &mut out_a[..rows * row_len_a];
    let mut rest_b = &mut out_b[..rows * row_len_b];
    std::thread::scope(|scope| {
        let mut y0 = 0;
        while y0 < rows {
            let take = band.min(rows - y0);
            let (chunk_a, tail_a) = rest_a.split_at_mut(take * row_len_a);
            let (chunk_b, tail_b) = rest_b.split_at_mut(take * row_len_b);
            rest_a = tail_a;
            rest_b = tail_b;
            let f = &f;
            scope.spawn(move || {
                let mut a = chunk_a;
                let mut b = chunk_b;
                for i in 0..take {
                    let (row_a, tail_a) = a.split_at_mut(row_len_a);
                    let (row_b, tail_b) = b.split_at_mut(row_len_b);
                    a = tail_a;
                    b = tail_b;
                    f(y0 + i, row_a, row_b);
                }
            });
            y0 += take;
        }
    });
}

/// 高斯核半径 = 3σ 时的可分卷积。
///
/// `sigma` 以像素为单位；`buffer` 为 `width * height * channels` 个 `f32`。
pub(crate) fn gaussian_blur(
    buffer: &mut [f32],
    width: usize,
    height: usize,
    channels: usize,
    sigma: f32,
) {
    if width == 0 || height == 0 || channels == 0 || sigma.is_nan() || sigma <= 0.05 {
        return;
    }
    if buffer.len() < width * height * channels {
        return;
    }

    let radius = (3.0 * sigma).ceil() as isize;
    if radius < 1 {
        return;
    }
    let taps = (2 * radius + 1) as usize;

    // 归一化的一维核（对称，只算一次）。
    let mut kernel = vec![0.0f32; taps];
    let mut total = 0.0f32;
    for (i, w) in kernel.iter_mut().enumerate() {
        let d = i as f32 - radius as f32;
        *w = (-0.5 * (d / sigma) * (d / sigma)).exp();
        total += *w;
    }
    let inv_total = 1.0 / total;
    for w in &mut kernel {
        *w *= inv_total;
    }

    let row_len = width * channels;
    let mut tmp = vec![0.0f32; buffer.len()];

    // 水平：tmp = buffer ⊗ kernel
    {
        let src = &buffer[..];
        par_rows_mut(&mut tmp, row_len, height, |y, out_row| {
            let row = &src[y * row_len..(y + 1) * row_len];
            for x in 0..width {
                for c in 0..channels {
                    let mut acc = 0.0f32;
                    for (i, kw) in kernel.iter().enumerate() {
                        let sx = (x as isize + i as isize - radius).clamp(0, width as isize - 1)
                            as usize;
                        acc += row[sx * channels + c] * kw;
                    }
                    out_row[x * channels + c] = acc;
                }
            }
        });
    }

    // 垂直：buffer = tmp ⊗ kernel
    {
        let src = &tmp[..];
        par_rows_mut(buffer, row_len, height, |y, out_row| {
            for x in 0..width {
                for c in 0..channels {
                    let mut acc = 0.0f32;
                    for (i, kw) in kernel.iter().enumerate() {
                        let sy = (y as isize + i as isize - radius).clamp(0, height as isize - 1)
                            as usize;
                        acc += src[(sy * width + x) * channels + c] * kw;
                    }
                    out_row[x * channels + c] = acc;
                }
            }
        });
    }
}

/// 双线性重采样（像素中心对齐）。通道数不变。
pub(crate) fn resize_bilinear(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    channels: usize,
) -> Vec<f32> {
    if src_w == dst_w && src_h == dst_h {
        return src.to_vec();
    }
    let mut out = vec![0.0f32; dst_w * dst_h * channels];
    let x_ratio = src_w as f32 / dst_w as f32;
    let y_ratio = src_h as f32 / dst_h as f32;

    par_rows_mut(&mut out, dst_w * channels, dst_h, |y, row| {
        let sy = ((y as f32 + 0.5) * y_ratio - 0.5).clamp(0.0, src_h as f32 - 1.0);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(src_h - 1);
        let fy = sy - y0 as f32;
        let row0 = y0 * src_w * channels;
        let row1 = y1 * src_w * channels;

        for x in 0..dst_w {
            let sx = ((x as f32 + 0.5) * x_ratio - 0.5).clamp(0.0, src_w as f32 - 1.0);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let fx = sx - x0 as f32;
            let i00 = row0 + x0 * channels;
            let i10 = row0 + x1 * channels;
            let i01 = row1 + x0 * channels;
            let i11 = row1 + x1 * channels;

            for c in 0..channels {
                let p00 = src[i00 + c];
                let p10 = src[i10 + c];
                let p01 = src[i01 + c];
                let p11 = src[i11 + c];
                let top = p00 + (p10 - p00) * fx;
                let bottom = p01 + (p11 - p01) * fx;
                row[x * channels + c] = top + (bottom - top) * fy;
            }
        }
    });
    out
}

/// Scharr 梯度（3×3，中心在 `(x, y)`，要求 `1 <= x < width-1`）。
#[inline]
pub(crate) fn scharr_gradient(src: &[f32], width: usize, x: usize, y: usize) -> f32 {
    let idx = |dx: isize, dy: isize| -> f32 {
        let xx = (x as isize + dx) as usize;
        let yy = (y as isize + dy) as usize;
        src[yy * width + xx]
    };

    // 47/255 与 162/255 是 Scharr 核的归一化系数。
    const A: f32 = 47.0 / 255.0;
    const B: f32 = 162.0 / 255.0;

    let gx = A * (idx(-1, -1) - idx(1, -1) + idx(-1, 1) - idx(1, 1)) + B * (idx(-1, 0) - idx(1, 0));
    let gy = A * (idx(-1, -1) - idx(-1, 1) + idx(1, -1) - idx(1, 1)) + B * (idx(0, -1) - idx(0, 1));

    (gx * gx + gy * gy).sqrt()
}

/// 导向滤波的降采样倍率：σ 越大越激进，上限 4。
pub(crate) fn get_scaling(sigma: f32) -> usize {
    let s = (sigma - 1.5).floor();
    if s < 1.0 {
        1
    } else if s > MAX_SCALING as f32 {
        MAX_SCALING
    } else {
        s as usize
    }
}

/// 2 通道导向图、2 通道输入的导向滤波。
///
/// `guide` 与 `input` 均为 `width * height * 2` 个 `f32`（U、V 交织）。
/// 返回同样布局的滤波结果：`out = a·guide + b`，其中 `a`、`b` 在
/// `(2σ+1)` 的窗口内取局部平均。
///
/// `input1_sigma_scale` 缩放第二输入通道（亮度）求局部均值时的 σ：
/// 亮度修正是加性的，参考实现用 `0.1σ` 让它保持局部，避免大半径把
/// 亮度整体搬走；色度通道传 `1.0`。
pub(crate) fn guided_filter_2ch(
    guide: &[f32],
    input: &[f32],
    width: usize,
    height: usize,
    sigma: f32,
    eps: f32,
    input1_sigma_scale: f32,
) -> Vec<f32> {
    let pixels = width * height;
    if pixels == 0 || guide.len() < pixels * 2 || input.len() < pixels * 2 {
        return input.to_vec();
    }

    let mut scaling = get_scaling(sigma).min(MAX_SCALING);
    // 太小的图不降采样，否则窗口没有意义。
    while scaling > 1 && (width / scaling < 2 || height / scaling < 2) {
        scaling -= 1;
    }
    let ds_w = (width / scaling).max(1);
    let ds_h = (height / scaling).max(1);
    let gsigma = (sigma / scaling as f32).max(0.2);
    let dn = ds_w * ds_h;

    let guide_ds = resize_bilinear(guide, width, height, ds_w, ds_h, 2);
    // 自导向（guide 与 input 是同一张图）时，均值与协方差可以复用，省两次模糊。
    let self_guided = std::ptr::eq(guide.as_ptr(), input.as_ptr());
    let input_ds = if self_guided {
        guide_ds.clone()
    } else {
        resize_bilinear(input, width, height, ds_w, ds_h, 2)
    };

    // 局部均值。
    let mut mean_g = guide_ds.clone();
    gaussian_blur(&mut mean_g, ds_w, ds_h, 2, gsigma);
    let mut mean_i = if self_guided {
        mean_g.clone()
    } else {
        input_ds.clone()
    };
    if !self_guided {
        gaussian_blur(&mut mean_i, ds_w, ds_h, 2, gsigma);
    }
    if (input1_sigma_scale - 1.0).abs() > 1e-6 && !self_guided {
        // 第二通道单独用更小的 σ 求均值。
        let mut channel: Vec<f32> = input_ds.iter().skip(1).step_by(2).copied().collect();
        gaussian_blur(&mut channel, ds_w, ds_h, 1, gsigma * input1_sigma_scale);
        for k in 0..dn {
            mean_i[k * 2 + 1] = channel[k];
        }
    }

    // 导向图协方差（3 个独立分量）与互协方差（4 个分量）。
    let mut cov = vec![0.0f32; dn * 3];
    let mut cross = vec![0.0f32; dn * 4];
    par_rows_mut2(
        &mut cov,
        ds_w * 3,
        &mut cross,
        ds_w * 4,
        ds_h,
        |y, cov_row, cross_row| {
            for x in 0..ds_w {
                let k = y * ds_w + x;
                let (u, v) = (guide_ds[k * 2], guide_ds[k * 2 + 1]);
                let (i0, i1) = (input_ds[k * 2], input_ds[k * 2 + 1]);

                cov_row[x * 3] = u * u;
                cov_row[x * 3 + 1] = u * v;
                cov_row[x * 3 + 2] = v * v;

                cross_row[x * 4] = u * i0;
                cross_row[x * 4 + 1] = v * i0;
                cross_row[x * 4 + 2] = u * i1;
                cross_row[x * 4 + 3] = v * i1;
            }
        },
    );
    gaussian_blur(&mut cov, ds_w, ds_h, 3, gsigma);
    if self_guided {
        // 自导向时互协方差就等于协方差矩阵本身。
        for k in 0..dn {
            cross[k * 4] = cov[k * 3];
            cross[k * 4 + 1] = cov[k * 3 + 1];
            cross[k * 4 + 2] = cov[k * 3 + 1];
            cross[k * 4 + 3] = cov[k * 3 + 2];
        }
    } else {
        gaussian_blur(&mut cross, ds_w, ds_h, 4, gsigma);
    }

    // a = 互协方差 · (协方差 + εI)⁻¹，b = 均值(input) − a·均值(guide)。
    let mut a = vec![0.0f32; dn * 4];
    let mut b = vec![0.0f32; dn * 2];
    par_rows_mut2(
        &mut a,
        ds_w * 4,
        &mut b,
        ds_w * 2,
        ds_h,
        |y, a_row, b_row| {
            for x in 0..ds_w {
                let k = y * ds_w + x;
                let mu = mean_g[k * 2];
                let mv = mean_g[k * 2 + 1];

                let suu = cov[k * 3] - mu * mu + eps;
                let suv = cov[k * 3 + 1] - mu * mv;
                let svv = cov[k * 3 + 2] - mv * mv + eps;

                let det = suu * svv - suv * suv;
                if det.abs() > 4.0 * f32::EPSILON {
                    let inv = [svv / det, -suv / det, -suv / det, suu / det];
                    // 互协方差：E[input·guide] − E[input]·E[guide]
                    let cu0 = cross[k * 4] - mu * mean_i[k * 2];
                    let cv0 = cross[k * 4 + 1] - mv * mean_i[k * 2];
                    let cu1 = cross[k * 4 + 2] - mu * mean_i[k * 2 + 1];
                    let cv1 = cross[k * 4 + 3] - mv * mean_i[k * 2 + 1];

                    a_row[x * 4] = cu0 * inv[0] + cv0 * inv[2];
                    a_row[x * 4 + 1] = cu0 * inv[1] + cv0 * inv[3];
                    a_row[x * 4 + 2] = cu1 * inv[0] + cv1 * inv[2];
                    a_row[x * 4 + 3] = cu1 * inv[1] + cv1 * inv[3];
                }

                b_row[x * 2] = mean_i[k * 2] - a_row[x * 4] * mu - a_row[x * 4 + 1] * mv;
                b_row[x * 2 + 1] =
                    mean_i[k * 2 + 1] - a_row[x * 4 + 2] * mu - a_row[x * 4 + 3] * mv;
            }
        },
    );

    gaussian_blur(&mut a, ds_w, ds_h, 4, gsigma);
    gaussian_blur(&mut b, ds_w, ds_h, 2, gsigma);

    let (a_full, b_full) = if scaling == 1 {
        (a, b)
    } else {
        (
            resize_bilinear(&a, ds_w, ds_h, width, height, 4),
            resize_bilinear(&b, ds_w, ds_h, width, height, 2),
        )
    };

    let mut out = vec![0.0f32; pixels * 2];
    par_rows_mut(&mut out, width * 2, height, |y, row| {
        for x in 0..width {
            let k = y * width + x;
            let (u, v) = (guide[k * 2], guide[k * 2 + 1]);
            row[x * 2] = a_full[k * 4] * u + a_full[k * 4 + 1] * v + b_full[k * 2];
            row[x * 2 + 1] = a_full[k * 4 + 2] * u + a_full[k * 4 + 3] * v + b_full[k * 2 + 1];
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_preserves_constant() {
        let mut buf = vec![0.37f32; 17 * 13];
        gaussian_blur(&mut buf, 17, 13, 1, 2.5);
        for v in &buf {
            assert!((v - 0.37).abs() < 1e-5);
        }
    }

    #[test]
    fn gaussian_smooths_impulse() {
        let (w, h) = (15, 15);
        let mut buf = vec![0.0f32; w * h];
        buf[7 * w + 7] = 1.0;
        gaussian_blur(&mut buf, w, h, 1, 1.5);
        // 峰值应低于 1，能量扩散到邻居。
        assert!(buf[7 * w + 7] < 1.0 && buf[7 * w + 7] > 0.0);
        assert!(buf[7 * w + 6] > 0.0);
    }

    #[test]
    fn resize_keeps_constant() {
        let src = vec![2.5f32; 8 * 6 * 2];
        let out = resize_bilinear(&src, 8, 6, 4, 3, 2);
        for v in out {
            assert!((v - 2.5).abs() < 1e-6);
        }
    }

    #[test]
    fn guided_filter_is_identity_on_linear_ramp() {
        // 导向图与输入都是线性渐变时，局部仿射模型可以完美重建。
        let (w, h) = (32, 32);
        let mut guide = vec![0.0f32; w * h * 2];
        let mut input = vec![0.0f32; w * h * 2];
        for y in 0..h {
            for x in 0..w {
                let k = (y * w + x) * 2;
                guide[k] = x as f32 * 0.1;
                guide[k + 1] = y as f32 * 0.1;
                input[k] = 0.5 + x as f32 * 0.2;
                input[k + 1] = 0.5 + y as f32 * 0.2;
            }
        }
        let out = guided_filter_2ch(&guide, &input, w, h, 3.0, 1e-6, 1.0);
        // 边界处的高斯均值被边缘复制拉偏（σ=3 → 半径 9），只检查内部。
        for y in 10..h - 10 {
            for x in 10..w - 10 {
                let k = y * w + x;
                assert!(
                    (out[k * 2] - input[k * 2]).abs() < 2e-3,
                    "{x},{y}: {} vs {}",
                    out[k * 2],
                    input[k * 2]
                );
                assert!((out[k * 2 + 1] - input[k * 2 + 1]).abs() < 2e-3, "{x},{y}");
            }
        }
    }

    #[test]
    fn scharr_is_zero_on_flat_field() {
        let src = vec![0.4f32; 9 * 9];
        assert!(scharr_gradient(&src, 9, 4, 4).abs() < 1e-6);
    }
}
