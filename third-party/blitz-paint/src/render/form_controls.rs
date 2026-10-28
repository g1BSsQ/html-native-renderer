// Modified from Blitz 92b11f1b: paint static check, swatch, slider and select decorations.
use super::ElementCx;
use crate::color::{Color, ToColorColor as _};
use anyrender::PaintScene;
use blitz_dom::local_name;
use kurbo::{Affine, BezPath, Cap, Circle, Join, Point, RoundedRect, Stroke, Vec2};
use peniko::Fill;
use style::dom::TElement as _;

/// CSS color-input values are #rrggbb; invalid values use black.
fn hex_swatch(value: Option<&str>) -> Color {
    let Some(value) = value.filter(|s| s.len() == 7 && s.starts_with('#')) else {
        return Color::from_rgba8(0, 0, 0, 255);
    };
    let rgb: Option<Vec<u8>> = (1..7).step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i+2], 16).ok())
        .collect();
    let Some(rgb) = rgb else { return Color::from_rgba8(0,0,0,255); };
    Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255)
}

impl ElementCx<'_, '_> {
    pub(super) fn draw_input(&self, scene: &mut impl PaintScene) {
        // A CSS appearance:none control keeps its author box without UA marks.
        if self.context.dom.resolved_style_value(self.node.id, "appearance") == "none" {
            return;
        }
        let name = self.node.local_name();
        let kind = self.node.attr(local_name!("type")).unwrap_or("text");
        let disabled = self.node.attr(local_name!("disabled")).is_some();
        let accent = if disabled { Color::from_rgba8(164,164,164,255) }
                     else { Color::from_rgba8(11,87,208,255) };
        let frame = self.frame.border_box;
        let width = frame.width();
        let center = frame.center();
        let scale = (width.min(frame.height()) - 2.0).max(0.0) / 16.0;

        if name == "select" {
            let size = self.node.attr(local_name!("size"))
                .and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
            if size <= 1 {
                let x = frame.x1 - 11.0 * self.scale;
                let y = center.y;
                let mut arrow = BezPath::new();
                arrow.move_to((x-4.0*self.scale, y-2.0*self.scale));
                arrow.line_to((x+4.0*self.scale, y-2.0*self.scale));
                arrow.line_to((x, y+2.5*self.scale));
                arrow.close_path();
                let color = if disabled { Color::from_rgba8(132,132,132,255) }
                            else { Color::from_rgba8(70,70,70,255) };
                scene.fill(Fill::NonZero, self.transform, color, None, &arrow);
            }
            return;
        }
        if name != "input" { return; }
        match kind {
            "checkbox" => {
                if let Some(checked) = self.element.checkbox_input_checked() {
                    draw_checkbox(scene, checked, frame.to_rounded_rect(2.0*self.scale),
                        self.transform, accent, scale);
                }
            }
            "radio" => {
                if let Some(checked) = self.element.checkbox_input_checked() {
                    draw_radio_button(scene, checked, center, self.transform, accent, scale);
                }
            }
            "range" => {
                let track = kurbo::Rect::new(frame.x0, center.y-2.0*self.scale,
                    frame.x1, center.y+2.0*self.scale).to_rounded_rect(2.0*self.scale);
                scene.fill(Fill::NonZero, self.transform,
                    Color::from_rgba8(200,200,200,255), None, &track);
                let raw = self.node.attr(local_name!("value"))
                    .and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(50.0);
                let min = self.node.attr(local_name!("min"))
                    .and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0);
                let max = self.node.attr(local_name!("max"))
                    .and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v > min).unwrap_or(100.0);
                let fraction = ((raw-min)/(max-min)).clamp(0.0,1.0);
                let radius = 7.0*self.scale;
                let x = frame.x0 + radius + fraction*(width-2.0*radius).max(0.0);
                scene.fill(Fill::NonZero, self.transform, accent, None,
                    &Circle::new((x,center.y), radius));
            }
            "color" => {
                let pad = 3.0*self.scale;
                let swatch = kurbo::Rect::new(frame.x0+pad,frame.y0+pad,frame.x1-pad,frame.y1-pad);
                if swatch.width()>0.0 && swatch.height()>0.0 {
                    scene.fill(Fill::NonZero, self.transform,
                        hex_swatch(self.node.attr(local_name!("value"))), None, &swatch);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hex_swatch;
    #[test]
    fn static_color_swatch_decodes_hex_bytes() {
        assert_eq!(hex_swatch(Some("#12abcd")), crate::color::Color::from_rgba8(18,171,205,255));
        assert_eq!(hex_swatch(Some("#invalid")), crate::color::Color::from_rgba8(0,0,0,255));
    }
}

fn draw_checkbox(
    scene: &mut impl PaintScene,
    checked: bool,
    frame: RoundedRect,
    transform: Affine,
    accent_color: Color,
    scale: f64,
) {
    if checked {
        scene.fill(Fill::NonZero, transform, accent_color, None, &frame);
        //Tick code derived from masonry
        let mut path = BezPath::new();
        path.move_to((2.0, 9.0));
        path.line_to((6.0, 13.0));
        path.line_to((14.0, 2.0));

        path.apply_affine(Affine::translate(Vec2 { x: 2.0, y: 1.0 }).then_scale(scale));

        let style = Stroke {
            width: 2.0 * scale,
            join: Join::Round,
            miter_limit: 10.0,
            start_cap: Cap::Round,
            end_cap: Cap::Round,
            dash_pattern: Default::default(),
            dash_offset: 0.0,
        };

        scene.stroke(&style, transform, Color::WHITE, None, &path);
    } else {
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &frame);
        scene.stroke(&Stroke::default(), transform, accent_color, None, &frame);
    }
}

fn draw_radio_button(
    scene: &mut impl PaintScene,
    checked: bool,
    center: Point,
    transform: Affine,
    accent_color: Color,
    scale: f64,
) {
    let outer_ring = Circle::new(center, 8.0 * scale);
    let gap = Circle::new(center, 6.0 * scale);
    let inner_circle = Circle::new(center, 4.0 * scale);
    if checked {
        scene.fill(Fill::NonZero, transform, accent_color, None, &outer_ring);
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &gap);
        scene.fill(Fill::NonZero, transform, accent_color, None, &inner_circle);
    } else {
        const GRAY: Color = color::palette::css::GRAY;
        scene.fill(Fill::NonZero, transform, GRAY, None, &outer_ring);
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &gap);
    }
}
