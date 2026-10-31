use crate::{ImageCacheConfig, VelloCpuScenePainter};
use anyrender::{ImageRenderer, RenderContext as AnyRenderContext};
use debug_timer::debug_timer;
use vello_cpu::{PixmapMut, RenderContext};

pub struct VelloCpuImageRenderer {
    scene: VelloCpuScenePainter,
}

impl VelloCpuImageRenderer {
    /// Render unchanged viewport-space drawing commands into a compact
    /// RGBA pixmap. The render context stays viewport-sized, so glyph/path
    /// encoding and the CPU fine-tile grid match a full-viewport render.
    /// Only the output pixmap is cropped, using the rasterizer's signed
    /// device-pixel offset rather than a translated scene transform.
    pub fn render_window<F: FnOnce(&mut VelloCpuScenePainter)>(
        &mut self,
        draw_fn: F,
        buffer: &mut [u8],
        target_width: u32,
        target_height: u32,
        origin_x: u32,
        origin_y: u32,
    ) {
        use vello_cpu::RasterizerSettings;
        assert_eq!(
            buffer.len(),
            target_width as usize * target_height as usize * 4,
        );
        draw_fn(&mut self.scene);
        self.scene.render_ctx.flush();
        self.scene.render_ctx.render_with(
            PixmapMut::new(target_width as u16, target_height as u16, buffer)
                .expect("valid compact raster buffer"),
            &mut self.scene.resources,
            RasterizerSettings {
                offset: (-(origin_x as i32), -(origin_y as i32)),
                ..Default::default()
            },
        );
        self.scene.maintain();
    }

    /// Create a renderer with a custom image cache configuration.
    pub fn with_image_cache_config(width: u32, height: u32, config: ImageCacheConfig) -> Self {
        Self {
            scene: VelloCpuScenePainter::with_image_cache_config(
                width as u16,
                height as u16,
                config,
            ),
        }
    }

    /// Enable or disable caching of rasterized glyphs in Vello's glyph atlas.
    ///
    /// Defaults to `false`.
    ///
    /// Note: Vello considers atlas-backed glyph caching experimental.
    pub fn set_glyph_caching(&mut self, enabled: bool) {
        self.scene.set_glyph_caching(enabled);
    }

    /// Drop all cached image conversions.
    pub fn clear_image_cache(&mut self) {
        self.scene.clear_image_cache();
    }
}

impl AnyRenderContext for VelloCpuImageRenderer {}
impl ImageRenderer for VelloCpuImageRenderer {
    type ScenePainter<'a> = VelloCpuScenePainter;

    fn new(width: u32, height: u32) -> Self {
        Self {
            scene: VelloCpuScenePainter::new(width as u16, height as u16),
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.scene.render_ctx = RenderContext::new(width as u16, height as u16);
    }

    fn reset(&mut self) {
        self.scene.render_ctx.reset();
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F, buffer: &mut [u8]) {
        debug_timer!(timer, feature = "log_frame_times");

        draw_fn(&mut self.scene);
        timer.record_time("cmds");

        self.scene.render_ctx.flush();
        timer.record_time("flush");

        self.scene.render_ctx.render(
            PixmapMut::new(
                self.scene.render_ctx.width(),
                self.scene.render_ctx.height(),
                buffer,
            )
            .unwrap(),
            &mut self.scene.resources,
        );
        timer.record_time("render");

        self.scene.maintain();
        timer.record_time("maintain");

        timer.print_times("vello_cpu: ");
    }

    fn render_to_vec<F: FnOnce(&mut Self::ScenePainter<'_>)>(
        &mut self,
        draw_fn: F,
        buffer: &mut Vec<u8>,
    ) {
        let width = self.scene.render_ctx.width();
        let height = self.scene.render_ctx.height();
        buffer.resize(width as usize * height as usize * 4, 0);
        self.render(draw_fn, buffer);
    }
}
