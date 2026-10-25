//! Device setup and readback helpers. The same code runs on Metal, Vulkan and DX12 natively
//! and on WebGPU in browsers.

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The device has `TIMESTAMP_QUERY`.
    pub timestamps: bool,
}

impl Gpu {
    /// Request the adapter's full limits (raised from the defaults) and timestamps if offered.
    pub async fn new(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: surface,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| format!("no adapter: {e}"))?;
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut features = wgpu::Features::empty();
        if timestamps {
            features |= wgpu::Features::TIMESTAMP_QUERY;
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("vxr"),
                required_features: features,
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("no device: {e}"))?;
        Ok(Self { instance, adapter, device, queue, timestamps })
    }
}

/// Map a `MAP_READ` buffer and copy it out (blocking natively, awaiting the event loop on the web).
pub async fn map_read(device: &wgpu::Device, buf: &wgpu::Buffer) -> Vec<u8> {
    let (tx, rx) = futures_channel::oneshot::channel();
    buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    #[cfg(target_arch = "wasm32")]
    let _ = device;
    rx.await.expect("map callback dropped").expect("map failed");
    let data = buf.slice(..).get_mapped_range().expect("mapped range").to_vec();
    buf.unmap();
    data
}

/// Read a 4-byte-per-texel 2D texture back as tightly packed rows.
pub async fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let (w, h) = (texture.width(), texture.height());
    let row = w * 4;
    let padded = row.div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: padded as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    let data = map_read(device, &buf).await;
    let mut out = Vec::with_capacity((row * h) as usize);
    for y in 0..h as usize {
        out.extend_from_slice(&data[y * padded as usize..y * padded as usize + row as usize]);
    }
    out
}

/// Milliseconds since the first call (native) or since page load (web).
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

/// A texture read back without blocking: `start` copies it to a buffer and asks for a map,
/// `poll` returns the tightly packed rows once the map is done. Works the same natively and
/// in browsers (where nothing may wait on the GPU).
pub struct Readback {
    buf: wgpu::Buffer,
    ready: std::sync::Arc<std::sync::atomic::AtomicU8>,
    width: u32,
    height: u32,
    padded: u32,
}

impl Readback {
    pub fn start(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Self {
        use std::sync::atomic::{AtomicU8, Ordering};
        let (width, height) = (texture.width(), texture.height());
        let padded = (width * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        // 0 waiting, 1 mapped, 2 failed.
        let ready = std::sync::Arc::new(AtomicU8::new(0));
        let r = ready.clone();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |res| {
            r.store(if res.is_ok() { 1 } else { 2 }, Ordering::Release);
        });
        Self { buf, ready, width, height, padded }
    }

    /// The rows once mapped, `Some(Err)` if the map failed, `None` while still waiting.
    pub fn poll(&self, device: &wgpu::Device) -> Option<Result<Vec<u8>, String>> {
        use std::sync::atomic::Ordering;
        #[cfg(not(target_arch = "wasm32"))]
        let _ = device.poll(wgpu::PollType::Poll);
        #[cfg(target_arch = "wasm32")]
        let _ = device;
        match self.ready.load(Ordering::Acquire) {
            0 => None,
            2 => Some(Err("the GPU readback failed".into())),
            _ => {
                let row = (self.width * 4) as usize;
                let data = self.buf.slice(..).get_mapped_range().expect("mapped range");
                let mut out = Vec::with_capacity(row * self.height as usize);
                for y in 0..self.height as usize {
                    let at = y * self.padded as usize;
                    out.extend_from_slice(&data[at..at + row]);
                }
                drop(data);
                self.buf.unmap();
                Some(Ok(out))
            }
        }
    }
}
