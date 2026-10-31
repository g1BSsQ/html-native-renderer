use super::ElementCx;
use crate::kurbo_css::{CssBox, NonUniformRoundedRectRadii};
use crate::color::{Color, ToColorColor as _};
use anyrender::PaintScene;
use kurbo::{Insets, Rect, Vec2};
use peniko::{Compose, Fill, Mix};

impl ElementCx<'_, '_> {
    pub(super) fn draw_outset_box_shadow(&self, scene: &mut impl PaintScene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let has_outset_shadow = box_shadow.iter().any(|s| !s.inset);
        if !has_outset_shadow {
            return;
        }

        let current_color = self.style.clone_color();
        let opacity = self.style.get_effects().opacity;
        let bg_color = self
            .style
            .get_background()
            .background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();
        let bg_is_opaque = bg_color.components[3] >= 1.0;
        let needs_clip = opacity < 1.0 || !bg_is_opaque;

        let max_shadow_rect = box_shadow.iter().fold(Rect::ZERO, |prev, shadow| {
            let x = shadow.base.horizontal.px() as f64 * self.scale;
            let y = shadow.base.vertical.px() as f64 * self.scale;
            let blur = shadow.base.blur.px() as f64 * self.scale;
            let spread = shadow.spread.px() as f64 * self.scale;
            let offset = spread + blur * 2.5;

            let rect = self.frame.border_box.inflate(offset, offset) + Vec2::new(x, y);

            prev.union(rect)
        });

        self.context.layer_manager.maybe_with_layer(
            scene,
            needs_clip,
            1.0,
            self.transform,
            &self.frame.shadow_clip(max_shadow_rect),
            None,
            None,
            |scene| {
                for shadow in box_shadow.iter().filter(|s| !s.inset).rev() {
                    let shadow_color = shadow
                        .base
                        .color
                        .resolve_to_absolute(&current_color)
                        .as_srgb_color();

                    let alpha = shadow_color.components[3];
                    if alpha != 0.0 {
                        let transform = self.transform.then_translate(Vec2 {
                            x: shadow.base.horizontal.px() as f64 * self.scale,
                            y: shadow.base.vertical.px() as f64 * self.scale,
                        });

                        let spread = shadow.spread.px() as f64 * self.scale;
                        let blur = shadow.base.blur.px() as f64;

                        // An unblurred shadow is a spread-adjusted copy of the
                        // element's rounded border outline. Keep each corner's two
                        // radii rather than collapsing elliptical corners to an
                        // average circular radius (CSS Backgrounds 3 §6.1).
                        if blur == 0.0 {
                            let rect = self.frame.border_box.inflate(spread, spread);
                            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                                continue;
                            }
                            let adjust = |r: Vec2| Vec2::new(
                                (r.x + spread).max(0.0),
                                (r.y + spread).max(0.0),
                            );
                            let original = self.frame.border_radii;
                            let radii = NonUniformRoundedRectRadii {
                                top_left: adjust(original.top_left),
                                top_right: adjust(original.top_right),
                                bottom_right: adjust(original.bottom_right),
                                bottom_left: adjust(original.bottom_left),
                            };
                            let frame = CssBox::new(
                                rect, Insets::ZERO, Insets::ZERO, 0.0, radii,
                            );
                            scene.fill(
                                Fill::NonZero, transform, shadow_color, None,
                                &frame.border_box_path(),
                            );
                            continue;
                        }

                        // Blurred shadows currently use the renderer's single-radius brush.
                        let radius = self.frame.border_radii.average();
                        let rect = self.frame.border_box.inflate(spread, spread);

                        // Fill the color
                        scene.draw_box_shadow(transform, rect, shadow_color, radius, blur);
                    }
                }
            },
        )
    }

    pub(super) fn draw_inset_box_shadow(&self, scene: &mut impl PaintScene) {
        let current_color = self.style.clone_color();
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let has_inset_shadow = box_shadow.iter().any(|s| s.inset);
        if !has_inset_shadow {
            return;
        }

        let padding_box = self.frame.padding_box_path();

        // Earlier listed shadows are in front of later ones.
        for shadow in box_shadow.iter().filter(|s| s.inset).rev() {
            let shadow_color = shadow
                .base
                .color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            if shadow_color == Color::TRANSPARENT {
                continue;
            }

            // TODO draw shadows with matching individual radii instead of averaging
            let radius = self.frame.border_radii.average();
            let transform = self.transform.then_translate(Vec2 {
                x: shadow.base.horizontal.px() as f64 * self.scale,
                y: shadow.base.vertical.px() as f64 * self.scale,
            });

            scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                1.0,
                self.transform,
                &padding_box,
                None,
                None,
            );
            scene.fill(
                Fill::NonZero,
                self.transform,
                shadow_color,
                None,
                &padding_box,
            );

            scene.push_layer(
                Fill::NonZero,
                Compose::DestOut,
                1.0,
                self.transform,
                &padding_box,
                None,
                None,
            );
            // The unshadowed area is the padding box shrunk by the spread radius
            let spread = shadow.spread.px() as f64 * self.scale;
            let hole = self.frame.padding_box.inflate(-spread, -spread);
            scene.draw_box_shadow(
                transform,
                hole,
                Color::WHITE,
                radius,
                shadow.base.blur.px() as f64 * self.scale,
            );

            scene.pop_layer();
            scene.pop_layer();
        }
    }
}
