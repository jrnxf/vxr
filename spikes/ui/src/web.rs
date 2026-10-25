//! Browser viewer for the HTML/CSS build: the test 1 renderer on a canvas, orbit and zoom, and
//! single-voxel add, erase, paint and pick. The page draws the Hybrid UI in the DOM over it.

use vxr_engine_spike::{bench::TARGET_FORMAT, camera::OrbitCamera, gpu, scene::Scene};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Viewer {
    pub(crate) gpu: gpu::Gpu,
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) scene: Scene,
    pub(crate) map: gpu::GpuBrickMap,
    pub(crate) renderer: gpu::Renderer,
    pub(crate) cam: OrbitCamera,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

fn err(e: impl ToString) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Open the `.vox` bytes on `canvas` (sized in device pixels by the caller).
#[wasm_bindgen]
pub async fn create_viewer(canvas: web_sys::HtmlCanvasElement, vox: Vec<u8>) -> Result<Viewer, JsValue> {
    console_error_panic_hook::set_once();
    let (width, height) = (canvas.width().max(1), canvas.height().max(1));
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas)).map_err(err)?;
    let g = gpu::Gpu::new(instance, Some(&surface)).await?;
    let mut scene = vxr_engine_spike::scene::from_vox("scene", &vox)?;
    scene.map.track_dirty(true);
    scene.map.take_dirty();
    let layout = gpu::brickmap_layout(&g.device);
    let map = gpu::GpuBrickMap::new_with_layout(&g.device, &g.queue, &layout, &scene.map, &scene.palette, gpu::MapLayout::Compact)?;
    let renderer = gpu::Renderer::new(&g.device, &layout, TARGET_FORMAT, width, height, gpu::RenderOptions { gbuffer: false, levels: 4 });
    let mut v = Viewer { gpu: g, surface, scene, map, renderer, cam: OrbitCamera::framing(glam::Vec3::ZERO, glam::Vec3::ONE), width, height };
    v.configure();
    v.frame_scene();
    Ok(v)
}

#[wasm_bindgen]
impl Viewer {
    fn configure(&self) {
        self.surface.configure(
            &self.gpu.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: TARGET_FORMAT,
                color_space: Default::default(),
                width: self.width,
                height: self.height,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
            },
        );
    }

    /// Canvas size in device pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return;
        }
        (self.width, self.height) = (width, height);
        self.renderer.resize(&self.gpu.device, width, height);
        self.configure();
    }

    pub fn frame_scene(&mut self) {
        let (lo, hi) = self.scene.map.occupied_bounds().unwrap_or(([0; 3], [8; 3]));
        let v = |a: [u32; 3]| glam::Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        self.cam = OrbitCamera::framing_aspect(v(lo), v(hi), self.width as f32 / self.height as f32);
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.cam.orbit(dx, dy);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.cam.pan(dx, dy, self.height as f32);
    }

    /// Zoom toward the voxel under device pixel (x, y), or the target if there is none.
    pub fn zoom(&mut self, steps: f32, x: f32, y: f32) {
        let (ro, dir) = self.cam.ray(x, y, self.width as f32, self.height as f32);
        let hit = self.scene.map.raycast(ro.into(), dir.into(), 1e6).map(|h| (dir, h.t));
        self.cam.zoom(steps, hit);
    }

    /// Apply `tool` (0 add, 1 erase, 2 paint, 3 pick) with palette `slot` at device pixel (x, y)
    /// and upload the change, so the next `frame` shows it. Returns the voxel's palette slot for
    /// pick, 1 if a voxel changed otherwise, 0 if nothing changed, and -1 on a miss.
    pub fn edit(&mut self, x: f32, y: f32, tool: u32, slot: u8) -> i32 {
        let Some(hit) = self.hit(x, y) else { return -1 };
        let [vx, vy, vz] = hit.voxel;
        let [nx, ny, nz] = hit.normal;
        let changed = match tool {
            0 => self.scene.map.set(vx + nx, vy + ny, vz + nz, slot),
            1 => self.scene.map.set(vx, vy, vz, 0),
            2 => self.scene.map.set(vx, vy, vz, slot),
            _ => return hit.value as i32,
        };
        if changed && let Err(e) = self.map.upload_dirty(&self.gpu.device, &self.gpu.queue, &mut self.scene.map, &self.scene.palette) {
            web_sys::console::error_1(&err(e));
        }
        changed as i32
    }

    /// The voxel under device pixel (x, y) as [x, y, z], or empty on a miss.
    pub fn cursor(&self, x: f32, y: f32) -> Vec<i32> {
        self.hit(x, y).map(|h| h.voxel.to_vec()).unwrap_or_default()
    }

    /// March and present one frame.
    pub fn frame(&mut self) {
        self.frame_with(|_, _, _| ());
    }

    pub fn voxels(&self) -> u32 {
        self.scene.map.voxels as u32
    }

    /// RGBA8 per palette slot (r in the low byte), 256 entries.
    pub fn palette(&self) -> Vec<u32> {
        self.scene.palette.to_vec()
    }

    /// One byte per palette slot: 1 if any voxel uses it.
    pub fn used(&self) -> Vec<u8> {
        let mut used = vec![0u8; 256];
        let s = self.scene.map.size_voxels();
        for z in 0..s[2] as i32 {
            for y in 0..s[1] as i32 {
                for x in 0..s[0] as i32 {
                    used[self.scene.map.get(x, y, z) as usize] = 1;
                }
            }
        }
        used[0] = 0;
        used
    }
}

impl Viewer {
    /// March and blit into the canvas, let `ui` record more passes over the frame, then present.
    pub(crate) fn frame_with(&mut self, ui: impl FnOnce(&gpu::Gpu, &mut wgpu::CommandEncoder, &wgpu::TextureView)) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                self.configure();
                return;
            }
        };
        self.renderer.set_camera(&self.gpu.queue, &self.cam);
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.gpu.device.create_command_encoder(&Default::default());
        self.renderer.march(&mut enc, &self.map, None);
        self.renderer.blit(&mut enc, &view, None);
        ui(&self.gpu, &mut enc, &view);
        self.gpu.queue.submit([enc.finish()]);
        self.gpu.queue.present(frame);
    }

    /// Ray from device pixel (x, y) into the scene.
    pub(crate) fn hit(&self, x: f32, y: f32) -> Option<vxr_engine_spike::brickmap::Hit> {
        let (ro, dir) = self.cam.ray(x, y, self.width as f32, self.height as f32);
        self.scene.map.raycast(ro.into(), dir.into(), 1e6)
    }
}
