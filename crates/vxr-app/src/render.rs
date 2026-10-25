//! The Render panel's settings and the PNG render export: the same path tracer as the corner,
//! at the export size, traced a slice per frame so editing never blocks, then read back
//! without waiting and encoded as PNG.

use glam::Vec3;
use vxr_render::gpu::Readback;
use vxr_render::map::srgb_to_linear;
use vxr_render::{EditBox, Gpu, GpuMap, Lighting, OrbitCamera, Preview};

/// Samples per pixel the corner stops at and an export traces.
pub const SPP_CHOICES: [u32; 4] = [64, 256, 1024, 4096];
/// Export sizes, all 16:9 like the corner, so an export frames what the corner shows.
pub const EXPORT_SIZES: [(u32, u32); 4] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
/// GPU time per frame an export may use.
const EXPORT_BUDGET_MS: f64 = 12.0;

/// What the Render panel edits. Angles in degrees; blur and focus are relative to the orbit
/// distance so they mean the same at any zoom.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderSettings {
    /// Sun direction around the vertical axis and above the horizon.
    pub sun_angle: f32,
    pub sun_height: f32,
    /// Sun colour (sRGB) and strength.
    pub sun_color: [u8; 3],
    pub sun_strength: f32,
    /// Sky light strength, and whether the sky shows behind the scene.
    pub sky: f32,
    pub sky_backdrop: bool,
    /// Exposure in stops.
    pub exposure_ev: f32,
    /// Depth of field: 0 is off, 1 is the strongest blur.
    pub blur: f32,
    /// Focus distance as a multiple of the orbit distance (1 focuses on the orbit target).
    pub focus: f32,
    /// Index into `SPP_CHOICES` and `EXPORT_SIZES`.
    pub spp: usize,
    pub size: usize,
}

impl Default for RenderSettings {
    fn default() -> Self {
        // The preview's default light (sun toward (0.45, 0.8, 0.35), irradiance (3, 2.85, 2.6)).
        Self {
            sun_angle: 38.0,
            sun_height: 54.0,
            sun_color: [255, 250, 241],
            sun_strength: 3.0,
            sky: 1.0,
            sky_backdrop: false,
            exposure_ev: 0.0,
            blur: 0.0,
            focus: 1.0,
            spp: 2,
            size: 1,
        }
    }
}

impl RenderSettings {
    /// The preview's lighting for a camera `distance` voxels from its target.
    pub fn lighting(&self, distance: f32) -> Lighting {
        let (a, h) = (self.sun_angle.to_radians(), self.sun_height.to_radians());
        let c = self.sun_color.map(srgb_to_linear);
        Lighting {
            sun_dir: Vec3::new(a.cos() * h.cos(), h.sin(), a.sin() * h.cos()),
            sun_color: Vec3::from(c) * self.sun_strength,
            sky: self.sky,
            sky_backdrop: self.sky_backdrop,
            exposure: 2f32.powf(self.exposure_ev),
            aperture: self.blur * 0.03 * distance,
            focus: self.focus * distance,
        }
    }

    pub fn samples(&self) -> u32 {
        SPP_CHOICES[self.spp.min(SPP_CHOICES.len() - 1)]
    }

    pub fn export_size(&self) -> (u32, u32) {
        EXPORT_SIZES[self.size.min(EXPORT_SIZES.len() - 1)]
    }
}

/// A render export in progress.
pub struct Export {
    preview: Preview,
    /// The camera when the export started (orbiting meanwhile does not restart it).
    cam: OrbitCamera,
    readback: Option<Readback>,
}

impl Export {
    pub fn start(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        palette: &[[u8; 4]; 256],
        cam: OrbitCamera,
        settings: &RenderSettings,
    ) -> Result<Self, String> {
        let (w, h) = settings.export_size();
        if !Preview::fits(&gpu.device, w, h) {
            return Err(format!("{w}×{h} is too large for this GPU"));
        }
        let mut preview = Preview::sized(&gpu.device, &gpu.queue, layout, palette, gpu.timestamps, (w, h));
        preview.lighting = settings.lighting(cam.distance);
        preview.max_spp = settings.samples();
        Ok(Self { preview, cam, readback: None })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.preview.width, self.preview.height)
    }

    /// GPU time of one full sample at the export size, and whether it was measured.
    pub fn sample_ms(&self) -> (f64, bool) {
        self.preview.sample_ms()
    }

    /// Samples per pixel so far and the target.
    pub fn progress(&self) -> (f32, u32) {
        (self.preview.samples().min(self.preview.max_spp as f32), self.preview.max_spp)
    }

    pub fn set_palette(&mut self, queue: &wgpu::Queue, palette: &[[u8; 4]; 256]) {
        self.preview.set_palette(queue, palette);
    }

    /// Encode this frame's slice of the export, unless it is done tracing.
    pub fn encode(&mut self, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, map: &GpuMap, edit: Option<EditBox>) {
        if self.readback.is_none() {
            self.preview.encode_budget(queue, enc, map, &self.cam, edit, EXPORT_BUDGET_MS);
        }
    }

    /// After the frame's submit: start reading back once traced, and return the PNG once read.
    pub fn after_submit(&mut self, gpu: &Gpu) -> Option<Result<Vec<u8>, String>> {
        match &self.readback {
            None => {
                self.preview.after_submit(&gpu.device);
                if self.preview.converged() {
                    self.readback = Some(Readback::start(&gpu.device, &gpu.queue, &self.preview.out_tex));
                }
                None
            }
            Some(r) => {
                let (w, h) = self.size();
                r.poll(&gpu.device).map(|rows| rows.and_then(|rgba| png(w, h, &rgba)))
            }
        }
    }
}

/// RGBA8 rows as a PNG file.
pub fn png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().and_then(|mut wr| wr.write_image_data(rgba)).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_match_the_preview_default() {
        let l = RenderSettings::default().lighting(100.0);
        let d = Lighting::default();
        assert!(l.sun_dir.angle_between(d.sun_dir) < 0.02, "{:?}", l.sun_dir);
        assert!((l.sun_color - d.sun_color).abs().max_element() < 0.05, "{:?}", l.sun_color);
        assert_eq!((l.exposure, l.aperture, l.focus, l.sky), (1.0, 0.0, 100.0, 1.0));
    }

    #[test]
    fn png_round_trips() {
        let rgba: Vec<u8> = (0..4 * 3 * 2).map(|i| i as u8 * 10).collect();
        let bytes = png(3, 2, &rgba).unwrap();
        let mut r = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(&buf[..info.buffer_size()], &rgba[..]);
    }
}
