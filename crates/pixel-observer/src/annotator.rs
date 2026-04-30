//! Screenshot annotator: draws red bounding boxes with white numeric labels
//! on top of a raw PNG and emits the corresponding [`AnnotatedElement`] map.
//!
//! The output is what Constraint and Agent modes feed to the model — the
//! model then emits actions referencing the numeric IDs (`CLICK 5`, etc.).
//!
//! No external font asset is bundled: digits are rendered from a hand-coded
//! 5x7 bitmap, which is plenty legible at the scale labels are drawn.

use image::{Rgba, RgbaImage};
use orchyn_core::action::{AnnotatedElement, BoundingBox};
use orchyn_core::{OrchynError, Result, UiElement};

const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

const DIGIT_W: u32 = 5;
const DIGIT_H: u32 = 7;
const LABEL_PAD: u32 = 2;

/// Draws numbered overlays on `raw_png` for every supplied [`UiElement`] and
/// returns `(annotated_png, element_map)`.
///
/// IDs are assigned in the order elements appear, starting at 1. Elements
/// past 99 are dropped so the labels stay 1–2 digits wide.
pub fn annotate_screenshot(
    raw_png: &[u8],
    elements: &[UiElement],
) -> Result<(Vec<u8>, Vec<AnnotatedElement>)> {
    let mut img = image::load_from_memory(raw_png)
        .map_err(|e| OrchynError::Internal(format!("annotate: load_from_memory: {e}")))?
        .to_rgba8();
    let (img_w, img_h) = img.dimensions();

    let mut annotated = Vec::with_capacity(elements.len().min(99));

    for (idx, el) in elements.iter().take(99).enumerate() {
        let id = (idx + 1) as u8;
        let bb = clamp_bounds(&el.bounds, img_w, img_h);

        draw_hollow_rect(&mut img, &bb, RED, 2);
        draw_label(&mut img, bb.x, bb.y, id);

        annotated.push(AnnotatedElement {
            id,
            element_type: el.element_type.clone(),
            text: el.text.clone(),
            bounds: bb,
        });
    }

    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| OrchynError::Internal(format!("annotate: encode: {e}")))?;
    Ok((buf, annotated))
}

fn clamp_bounds(bb: &BoundingBox, img_w: u32, img_h: u32) -> BoundingBox {
    let x = bb.x.min(img_w.saturating_sub(1));
    let y = bb.y.min(img_h.saturating_sub(1));
    let width = bb.width.min(img_w.saturating_sub(x));
    let height = bb.height.min(img_h.saturating_sub(y));
    BoundingBox { x, y, width, height }
}

fn draw_hollow_rect(img: &mut RgbaImage, bb: &BoundingBox, color: Rgba<u8>, thickness: u32) {
    let (img_w, img_h) = img.dimensions();
    if bb.width == 0 || bb.height == 0 {
        return;
    }
    let x0 = bb.x;
    let y0 = bb.y;
    let x1 = (bb.x + bb.width).min(img_w);
    let y1 = (bb.y + bb.height).min(img_h);

    for t in 0..thickness {
        // top + bottom
        for x in x0..x1 {
            put_pixel(img, x, y0 + t, color);
            if y1 > t + 1 {
                put_pixel(img, x, y1 - 1 - t, color);
            }
        }
        // left + right
        for y in y0..y1 {
            put_pixel(img, x0 + t, y, color);
            if x1 > t + 1 {
                put_pixel(img, x1 - 1 - t, y, color);
            }
        }
    }
}

fn draw_filled_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    let (img_w, img_h) = img.dimensions();
    let x1 = (x + w).min(img_w);
    let y1 = (y + h).min(img_h);
    for yy in y..y1 {
        for xx in x..x1 {
            put_pixel(img, xx, yy, color);
        }
    }
}

fn put_pixel(img: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>) {
    let (w, h) = img.dimensions();
    if x < w && y < h {
        img.put_pixel(x, y, color);
    }
}

/// Draws a red label background containing the white-numbered ID, placed just
/// above the top-left corner of the box (and clamped inside the image).
fn draw_label(img: &mut RgbaImage, anchor_x: u32, anchor_y: u32, id: u8) {
    let digits: Vec<u8> = if id >= 10 { vec![id / 10, id % 10] } else { vec![id] };
    let label_w = (digits.len() as u32) * DIGIT_W + (digits.len() as u32 - 1).max(0) + LABEL_PAD * 2;
    let label_h = DIGIT_H + LABEL_PAD * 2;

    // Place label above the box if there's room, otherwise inside the top-left.
    let label_y = anchor_y.checked_sub(label_h).unwrap_or(anchor_y);
    let label_x = anchor_x;

    draw_filled_rect(img, label_x, label_y, label_w, label_h, RED);

    let mut x_cursor = label_x + LABEL_PAD;
    let y_cursor = label_y + LABEL_PAD;
    for d in digits {
        draw_digit(img, x_cursor, y_cursor, d, WHITE);
        x_cursor += DIGIT_W + 1;
    }
}

/// 5x7 bitmap digits. Each row is 5 bits, MSB on the left.
const DIGIT_BITMAPS: [[u8; 7]; 10] = [
    // 0
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    // 1
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    // 2
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    // 3
    [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
    // 4
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    // 5
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    // 6
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    // 7
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    // 8
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    // 9
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

fn draw_digit(img: &mut RgbaImage, x: u32, y: u32, digit: u8, color: Rgba<u8>) {
    let bitmap = &DIGIT_BITMAPS[(digit as usize).min(9)];
    for (row, bits) in bitmap.iter().enumerate() {
        for col in 0..DIGIT_W {
            let mask = 1u8 << (DIGIT_W - 1 - col);
            if bits & mask != 0 {
                put_pixel(img, x + col, y + row as u32, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn annotates_assigns_sequential_ids() {
        let png = blank_png(200, 200);
        let elements = vec![
            UiElement {
                element_type: "button".into(),
                text: Some("Save".into()),
                bounds: BoundingBox { x: 10, y: 50, width: 60, height: 30 },
            },
            UiElement {
                element_type: "input".into(),
                text: None,
                bounds: BoundingBox { x: 10, y: 100, width: 100, height: 24 },
            },
        ];
        let (out, map) = annotate_screenshot(&png, &elements).unwrap();
        assert!(!out.is_empty());
        assert_eq!(map.len(), 2);
        assert_eq!(map[0].id, 1);
        assert_eq!(map[1].id, 2);
        assert_eq!(map[0].text.as_deref(), Some("Save"));
    }

    #[test]
    fn annotates_caps_at_99_elements() {
        let png = blank_png(50, 50);
        let elements: Vec<UiElement> = (0..150)
            .map(|_| UiElement {
                element_type: "x".into(),
                text: None,
                bounds: BoundingBox { x: 0, y: 0, width: 5, height: 5 },
            })
            .collect();
        let (_, map) = annotate_screenshot(&png, &elements).unwrap();
        assert_eq!(map.len(), 99);
    }

    #[test]
    fn annotates_clamps_oversized_bounds() {
        let png = blank_png(100, 100);
        let elements = vec![UiElement {
            element_type: "button".into(),
            text: None,
            bounds: BoundingBox { x: 80, y: 80, width: 500, height: 500 },
        }];
        let (_, map) = annotate_screenshot(&png, &elements).unwrap();
        assert_eq!(map[0].bounds.x, 80);
        assert_eq!(map[0].bounds.y, 80);
        assert_eq!(map[0].bounds.width, 20);
        assert_eq!(map[0].bounds.height, 20);
    }
}
