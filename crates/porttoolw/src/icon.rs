//! PortTool 图标：圆角方块中的双向连接线，托盘图标附带状态点。
//!
//! 用有向距离场逐像素计算覆盖率，任意尺寸都能 1:1 绘制，不需要位图资源。

use windui::prelude::WindowIcon;

const BG: [u8; 3] = [0x25, 0x63, 0xEB];
const FG: [u8; 3] = [0xFF, 0xFF, 0xFF];
const DOT_ON: [u8; 3] = [0x22, 0xC5, 0x5E];
const DOT_OFF: [u8; 3] = [0x94, 0xA3, 0xB8];

/// 状态点：有设备在线时绿色，否则灰色；`None` 表示不画状态点（窗口图标用）。
pub fn render(size: u32, online: Option<bool>) -> WindowIcon {
    let s = size.max(1) as f32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            // 归一化到 0..1 的像素中心坐标
            let p = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            let aa = 1.0 / s;
            let mut color = [0f32; 4];

            blend(&mut color, BG, coverage(rounded_rect(p, (0.5, 0.5), (0.46, 0.46), 0.2), aa));
            // 两条相反方向的线路与端点，代表不同类型端口之间的双向转发。
            for (a, b) in [
                ((0.28, 0.38), (0.70, 0.38)),
                ((0.58, 0.27), (0.70, 0.38)),
                ((0.58, 0.49), (0.70, 0.38)),
                ((0.30, 0.62), (0.72, 0.62)),
                ((0.42, 0.51), (0.30, 0.62)),
                ((0.42, 0.73), (0.30, 0.62)),
            ] {
                blend(&mut color, FG, coverage(segment(p, a, b, 0.038), aa));
            }
            for center in [(0.28, 0.38), (0.72, 0.62)] {
                blend(&mut color, FG, coverage(circle(p, center, 0.065), aa));
            }
            if let Some(on) = online {
                let ring = circle(p, (0.8, 0.8), 0.19);
                blend(&mut color, [0xFF, 0xFF, 0xFF], coverage(ring, aa));
                let dot = circle(p, (0.8, 0.8), 0.14);
                blend(&mut color, if on { DOT_ON } else { DOT_OFF }, coverage(dot, aa));
            }

            let i = ((y * size + x) * 4) as usize;
            let a = color[3];
            if a > 0.0 {
                for c in 0..3 {
                    rgba[i + c] = (color[c] / a * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                rgba[i + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    WindowIcon::from_rgba(size, size, rgba).expect("尺寸与像素数一致")
}

/// 预乘颜色上叠加一层。
fn blend(dst: &mut [f32; 4], rgb: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    for c in 0..3 {
        dst[c] = rgb[c] as f32 / 255.0 * alpha + dst[c] * (1.0 - alpha);
    }
    dst[3] = alpha + dst[3] * (1.0 - alpha);
}

/// 距离 → 覆盖率（边缘一个像素宽的抗锯齿）。
fn coverage(dist: f32, aa: f32) -> f32 {
    (0.5 - dist / aa).clamp(0.0, 1.0)
}

fn circle(p: (f32, f32), c: (f32, f32), r: f32) -> f32 {
    ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt() - r
}

fn rounded_rect(p: (f32, f32), c: (f32, f32), half: (f32, f32), r: f32) -> f32 {
    let qx = (p.0 - c.0).abs() - half.0 + r;
    let qy = (p.1 - c.1).abs() - half.1 + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// 带圆角端点的线段。
fn segment(p: (f32, f32), a: (f32, f32), b: (f32, f32), radius: f32) -> f32 {
    let (px, py) = (p.0 - a.0, p.1 - a.1);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - dx * t).powi(2) + (py - dy * t).powi(2)).sqrt() - radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_any_size() {
        for size in [16, 20, 32, 48, 256] {
            let icon = render(size, Some(true));
            assert_eq!(icon.rgba().len(), (size * size * 4) as usize);
        }
        // 角落透明，中心不透明
        let icon = render(32, None);
        assert_eq!(icon.rgba()[3], 0);
        let center = ((16 * 32 + 16) * 4 + 3) as usize;
        assert_eq!(icon.rgba()[center], 255);
    }
}
