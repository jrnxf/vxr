//! The Render panel's settings and the PNG render export: the same path tracer as the corner,
//! at the export size, traced a slice per frame so editing never blocks, then read back
//! without waiting and encoded as PNG.

use glam::Vec3;
use serde_json::{Value, json};
pub use vxr_core::files::png;
use vxr_core::material::Material;
use vxr_render::gpu::Readback;
use vxr_render::map::srgb_to_linear;
use vxr_render::{EditBox, Gpu, GpuMap, Lighting, OrbitCamera, Preview};

/// Samples per pixel the corner stops at and an export traces.
pub const SPP_CHOICES: [u32; 4] = [64, 256, 1024, 4096];
/// Export sizes, all 16:9 like the corner, so an export frames what the corner shows.
pub const EXPORT_SIZES: [(u32, u32); 4] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
/// The export sizes as an agent names them.
pub const EXPORT_SIZE_NAMES: [&str; 4] = ["720p", "1080p", "1440p", "4k"];
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
    /// Sky light strength, whether the sky shows behind the scene, and whether it is the
    /// physical sky (following the sun, with its disk) or the fixed gradient.
    pub sky: f32,
    pub sky_backdrop: bool,
    pub physical_sky: bool,
    /// Exposure in stops.
    pub exposure_ev: f32,
    /// Display gamma (2.2 is plain sRGB) and vignette (0 is off, 1 takes the corners to black).
    pub gamma: f32,
    pub vignette: f32,
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
            physical_sky: false,
            exposure_ev: 0.0,
            gamma: 2.2,
            vignette: 0.0,
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
            physical_sky: self.physical_sky,
            exposure: 2f32.powf(self.exposure_ev),
            gamma: self.gamma,
            vignette: self.vignette,
            aperture: self.blur * 0.03 * distance,
            focus: self.focus * distance,
        }
    }

    /// Set what an agent's `render_settings` call gives (the names `vxr_mcp::live` offers),
    /// all or nothing: a value out of the panel's range changes nothing.
    pub fn set_from(&mut self, args: &Value) -> Result<(), String> {
        let o = args.as_object().ok_or("the arguments: an object")?;
        let mut next = self.clone();
        for (k, v) in o {
            let num = |lo: f32, hi: f32| -> Result<f32, String> {
                let n = v.as_f64().ok_or_else(|| format!("{k}: a number"))? as f32;
                if (lo..=hi).contains(&n) { Ok(n) } else { Err(format!("{k} {n}: give {lo} to {hi}")) }
            };
            let flag = || v.as_bool().ok_or_else(|| format!("{k}: true or false"));
            match k.as_str() {
                "sun_direction" => next.sun_angle = num(0.0, 360.0)?,
                "sun_height" => next.sun_height = num(2.0, 90.0)?,
                "sun_strength" => next.sun_strength = num(0.0, 10.0)?,
                "sun_color" => {
                    let hex = v.as_str().ok_or("sun_color: #rrggbb")?;
                    next.sun_color = parse_hex(hex).ok_or_else(|| format!("sun_color {hex}: #rrggbb"))?;
                }
                "sky_light" => next.sky = num(0.0, 3.0)?,
                "show_sky" => next.sky_backdrop = flag()?,
                "physical_sky" => next.physical_sky = flag()?,
                "exposure" => next.exposure_ev = num(-3.0, 3.0)?,
                "gamma" => next.gamma = num(1.0, 3.0)?,
                "vignette" => next.vignette = num(0.0, 1.0)?,
                "depth_of_field" => next.blur = num(0.0, 1.0)?,
                "focus" => next.focus = num(0.25, 2.0)?,
                "samples" => {
                    let n = v.as_u64().unwrap_or(0);
                    next.spp = SPP_CHOICES
                        .iter()
                        .position(|&c| c as u64 == n)
                        .ok_or_else(|| format!("samples {v}: one of {SPP_CHOICES:?}"))?;
                }
                "export_size" => {
                    let n = v.as_str().unwrap_or("");
                    next.size = EXPORT_SIZE_NAMES
                        .iter()
                        .position(|&c| c.eq_ignore_ascii_case(n))
                        .ok_or_else(|| format!("export_size {v}: one of {EXPORT_SIZE_NAMES:?}"))?;
                }
                other => return Err(format!("{}: not a render setting", vxr_ops::clean(other.as_bytes()))),
            }
        }
        *self = next;
        Ok(())
    }

    /// The settings as an agent's `render_settings` call names them.
    pub fn to_json(&self) -> Value {
        let (w, h) = self.export_size();
        let c = self.sun_color;
        json!({
            "sun_direction": r3(self.sun_angle),
            "sun_height": r3(self.sun_height),
            "sun_color": format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
            "sun_strength": r3(self.sun_strength),
            "sky_light": r3(self.sky),
            "show_sky": self.sky_backdrop,
            "physical_sky": self.physical_sky,
            "exposure": r3(self.exposure_ev),
            "gamma": r3(self.gamma),
            "vignette": r3(self.vignette),
            "depth_of_field": r3(self.blur),
            "focus": r3(self.focus),
            "samples": self.samples(),
            "export_size": EXPORT_SIZE_NAMES[self.size.min(EXPORT_SIZES.len() - 1)],
            "export_pixels": [w, h],
        })
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
        materials: &[Material; 256],
        cam: OrbitCamera,
        settings: &RenderSettings,
        backdrop: [[u8; 3]; 2],
    ) -> Result<Self, String> {
        let (w, h) = settings.export_size();
        if !Preview::fits(&gpu.device, w, h) {
            return Err(format!("{w}×{h} is too large for this GPU"));
        }
        let mut preview = Preview::sized(&gpu.device, &gpu.queue, layout, palette, gpu.timestamps, (w, h));
        preview.set_materials(&gpu.queue, materials);
        preview.lighting = settings.lighting(cam.distance);
        preview.max_spp = settings.samples();
        // Only the finished image is shown, so no coarse first frame.
        preview.follow = false;
        preview.set_backdrop(backdrop);
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

    pub fn set_materials(&mut self, queue: &wgpu::Queue, materials: &[Material; 256]) {
        self.preview.set_materials(queue, materials);
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

/// A setting as an agent reads it: to three decimals, not f32's tail.
fn r3(v: f32) -> f64 {
    (v as f64 * 1000.0).round() / 1000.0
}

/// `#rrggbb` as sRGB.
fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_sets_every_panel_setting_all_or_nothing() {
        let mut r = RenderSettings::default();
        let all = json!({
            "sun_direction": 90, "sun_height": 30, "sun_color": "#ff8000", "sun_strength": 5,
            "sky_light": 2, "show_sky": true, "physical_sky": true, "exposure": -1.5, "gamma": 1.8,
            "vignette": 0.5, "depth_of_field": 0.25, "focus": 1.5, "samples": 1024, "export_size": "4K",
        });
        r.set_from(&all).unwrap();
        let want = RenderSettings {
            sun_angle: 90.0,
            sun_height: 30.0,
            sun_color: [255, 128, 0],
            sun_strength: 5.0,
            sky: 2.0,
            sky_backdrop: true,
            physical_sky: true,
            exposure_ev: -1.5,
            gamma: 1.8,
            vignette: 0.5,
            blur: 0.25,
            focus: 1.5,
            spp: 2,
            size: 3,
        };
        assert_eq!(r, want);
        let back = r.to_json();
        for (k, v) in all.as_object().unwrap() {
            if k != "export_size" {
                assert_eq!(
                    back[k].as_f64().or(back[k].as_bool().map(|b| b as u8 as f64)),
                    v.as_f64().or(v.as_bool().map(|b| b as u8 as f64)),
                    "{k}"
                );
            }
        }
        assert_eq!((back["export_size"].as_str(), back["export_pixels"][1].as_u64()), (Some("4k"), Some(2160)));
        // Set from its own description, nothing changes.
        let mut again = r.clone();
        let mut j = back.clone();
        j.as_object_mut().unwrap().remove("export_pixels");
        again.set_from(&j).unwrap();
        assert_eq!(again, r);
        for bad in [
            json!({"gamma": 1.5, "vignette": 2}),
            json!({"samples": 100}),
            json!({"export_size": "8k"}),
            json!({"sun_color": "red"}),
            json!({"show_sky": 1}),
            json!({"bloom": 1}),
        ] {
            assert!(r.set_from(&bad).is_err(), "{bad}");
            assert_eq!(r, want, "{bad} changed nothing");
        }
    }

    #[test]
    fn default_settings_match_the_preview_default() {
        let l = RenderSettings::default().lighting(100.0);
        let d = Lighting::default();
        assert!(l.sun_dir.angle_between(d.sun_dir) < 0.02, "{:?}", l.sun_dir);
        assert!((l.sun_color - d.sun_color).abs().max_element() < 0.05, "{:?}", l.sun_color);
        assert_eq!((l.exposure, l.aperture, l.focus, l.sky), (1.0, 0.0, 100.0, 1.0));
        assert_eq!((l.gamma, l.vignette, l.physical_sky), (d.gamma, d.vignette, d.physical_sky));
    }
}
