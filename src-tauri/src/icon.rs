use std::f32::consts::PI;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub const ICON_SIZE: u32 = 44;

pub fn render(remaining_ratio: f32, running: bool) -> Vec<u8> {
    let size = ICON_SIZE;
    let mut pixmap = Pixmap::new(size, size).expect("create pixmap");
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let outer_r = (size as f32 / 2.0) - 2.0;
    let tick_outer = outer_r - 0.5;
    let pie_r = outer_r - 4.0;

    fill_disc(&mut pixmap, cx, cy, outer_r);
    if remaining_ratio > 0.0 {
        draw_wedge(&mut pixmap, cx, cy, pie_r, remaining_ratio, running);
    }
    draw_ticks(&mut pixmap, cx, cy, tick_outer);
    stroke_outline(&mut pixmap, cx, cy, outer_r);

    unpremultiply(pixmap.take())
}

fn fill_disc(pixmap: &mut Pixmap, cx: f32, cy: f32, radius: f32) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 250, 243, 255);
    paint.anti_alias = true;

    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, radius);
    let path = pb.finish().expect("disc path");
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn stroke_outline(pixmap: &mut Pixmap, cx: f32, cy: f32, radius: f32) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(214, 207, 194, 255);
    paint.anti_alias = true;

    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, radius);
    let path = pb.finish().expect("outline path");

    let mut stroke = Stroke::default();
    stroke.width = 1.0;
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn draw_ticks(pixmap: &mut Pixmap, cx: f32, cy: f32, tick_outer: f32) {
    let mut major_paint = Paint::default();
    major_paint.set_color_rgba8(90, 84, 74, 255);
    major_paint.anti_alias = true;

    let mut minor_paint = Paint::default();
    minor_paint.set_color_rgba8(163, 157, 146, 255);
    minor_paint.anti_alias = true;

    let mut major_stroke = Stroke::default();
    major_stroke.width = 1.0;

    let mut minor_stroke = Stroke::default();
    minor_stroke.width = 0.6;

    for m in 0..60 {
        let is_major = m % 5 == 0;
        let angle = -PI / 2.0 + (m as f32 / 60.0) * 2.0 * PI;
        let len = if is_major { 2.6 } else { 1.4 };
        let inner = tick_outer - len;
        let x1 = cx + inner * angle.cos();
        let y1 = cy + inner * angle.sin();
        let x2 = cx + tick_outer * angle.cos();
        let y2 = cy + tick_outer * angle.sin();

        let mut pb = PathBuilder::new();
        pb.move_to(x1, y1);
        pb.line_to(x2, y2);
        let path = pb.finish().expect("tick path");

        let (paint, stroke) = if is_major {
            (&major_paint, &major_stroke)
        } else {
            (&minor_paint, &minor_stroke)
        };
        pixmap.stroke_path(&path, paint, stroke, Transform::identity(), None);
    }
}

fn draw_wedge(pixmap: &mut Pixmap, cx: f32, cy: f32, radius: f32, ratio: f32, running: bool) {
    let mut paint = Paint::default();
    if running {
        paint.set_color_rgba8(220, 60, 60, 255);
    } else {
        paint.set_color_rgba8(52, 199, 89, 255);
    }
    paint.anti_alias = true;

    let total_angle = 2.0 * PI * ratio.clamp(0.0, 1.0);
    let segments = 96_u32;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = -PI / 2.0 + total_angle * t;
        let x = cx + radius * angle.cos();
        let y = cy + radius * angle.sin();
        pb.line_to(x, y);
    }
    pb.close();
    let path = pb.finish().expect("wedge path");
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn unpremultiply(mut data: Vec<u8>) -> Vec<u8> {
    for chunk in data.chunks_exact_mut(4) {
        let a = chunk[3];
        if a == 0 || a == 255 {
            continue;
        }
        let af = a as f32 / 255.0;
        chunk[0] = ((chunk[0] as f32 / af).min(255.0)) as u8;
        chunk[1] = ((chunk[1] as f32 / af).min(255.0)) as u8;
        chunk[2] = ((chunk[2] as f32 / af).min(255.0)) as u8;
    }
    data
}
