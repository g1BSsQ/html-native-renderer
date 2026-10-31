use crate::{ImageCacheConfig, VelloCpuScenePainter};
use anyrender::{ImageRenderer, RenderContext as AnyRenderContext};
use debug_timer::debug_timer;
use vello_cpu::{PixmapMut, RenderContext};

pub struct VelloCpuImageRenderer {
    scene: VelloCpuScenePainter,
}

impl VelloCpuImageRenderer {
    /// Crop a viewport-space CPU scene into an aligned compact pixmap
    /// without translating geometry or shifting its fine-tile coordinates.
    /// The pinned local vello_cpu exposes signed raster destination offsets.
    pub fn render_window<F: FnOnce(&mut VelloCpuScenePainter)>(
        &mut self,
        draw_fn: F,
        buffer: &mut [u8],
        target_width: u32,
        target_height: u32,
        origin_x: u32,
        origin_y: u32,
    ) {
        let width = u16::try_from(target_width)
            .expect("compact raster width exceeds u16::MAX");
        let height = u16::try_from(target_height)
            .expect("compact raster height exceeds u16::MAX");
        assert_eq!(
            buffer.len(),
            usize::from(width) * usize::from(height) * 4,
        );
        draw_fn(&mut self.scene);
        self.scene.render_ctx.flush();
        self.scene.render_ctx.render_with(
            PixmapMut::new(width, height, buffer)
                .expect("valid compact raster buffer"),
            &mut self.scene.resources,
            vello_cpu::RasterizerSettings {
                offset: (-(i32::try_from(origin_x).expect("crop x exceeds i32::MAX")),
                         -(i32::try_from(origin_y).expect("crop y exceeds i32::MAX"))),
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
