//! GPU side: device setup, the brick pool upload (full and partial), the ray-march and blit
//! passes, and timestamp timing. The same code runs on Metal natively and WebGPU in browsers.

use crate::brickmap::{BrickMap, BRICK_VOXELS, EMPTY, MASK_WORDS, MAX_PAGES, PAGE_BRICKS, PAGE_SHIFT};
use crate::camera::OrbitCamera;
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub const BRICKMAP_WGSL: &str = include_str!("brickmap.wgsl");
pub const MARCH_WGSL: &str = include_str!("march.wgsl");
const BLIT_WGSL: &str = include_str!("blit.wgsl");

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
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
                label: Some("vxr-engine"),
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

/// Wait until all submitted GPU work is done (blocking natively, awaiting the event loop on the web).
pub async fn wait_idle(device: &wgpu::Device, queue: &wgpu::Queue) {
    let (tx, rx) = futures_channel::oneshot::channel();
    queue.on_submitted_work_done(move || {
        let _ = tx.send(());
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    #[cfg(target_arch = "wasm32")]
    let _ = device;
    let _ = rx.await;
}

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

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn brickmap_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let storage = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("brickmap"),
        entries: &[uniform(0), uniform(1), storage(2), storage(3), storage(4), storage(5), storage(6), storage(7), storage(8)],
    })
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MapInfo {
    bdims: [u32; 4],
    sdims: [u32; 4],
    lo: [f32; 4],
    hi: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct UploadStats {
    pub bricks: u32,
    pub cells: u32,
    pub coarse_cells: u32,
    pub bytes: u64,
    pub writes: u32,
    pub rebuilt: bool,
    /// Compacted layout: bricks whose heap block had to move (grew past its block, or
    /// shrank to under half of it).
    pub reallocs: u32,
}

/// How palette bytes are stored on the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapLayout {
    /// 512 bytes per brick, one per voxel slot, empty or not.
    #[default]
    Dense,
    /// Bytes only for occupied voxels, in 16-byte blocks of a heap, found by a per-brick
    /// offset plus a popcount of the mask bits below the voxel.
    Compact,
}

const HEAP_PAGE: u64 = 1 << 27; // 128 MiB, the default max storage binding
const BLOCK: u32 = 16;
const NO_BLOCK: u32 = u32::MAX;

/// CPU bookkeeping for the compacted heap: one block per live brick, free lists per block
/// size, bump allocation at the top. Blocks never cross a 128 MiB page.
struct CompactHeap {
    /// Two words per brick slot: heap byte offset, then the set-bit counts of mask words
    /// 0..4, 0..8 and 0..12 (9 bits each), so the shader's rank needs at most 4 popcounts.
    offsets: Vec<u32>,
    caps: Vec<u32>,
    free: Vec<Vec<u32>>,
    top: u64,
    capacity: u64,
}

impl CompactHeap {
    fn alloc(&mut self, cap: u32) -> Option<u32> {
        if let Some(o) = self.free[(cap / BLOCK) as usize].pop() {
            return Some(o);
        }
        let mut start = self.top;
        if start / HEAP_PAGE != (start + cap as u64 - 1) / HEAP_PAGE {
            start = (start / HEAP_PAGE + 1) * HEAP_PAGE;
        }
        if start + cap as u64 > self.capacity {
            return None;
        }
        self.top = start + cap as u64;
        Some(start as u32)
    }

    fn release(&mut self, b: usize) {
        if self.caps[b] > 0 {
            self.free[(self.caps[b] / BLOCK) as usize].push(self.offsets[b * 2]);
        }
        self.offsets[b * 2] = NO_BLOCK;
        self.offsets[b * 2 + 1] = 0;
        self.caps[b] = 0;
    }

    fn live_block_bytes(&self) -> u64 {
        self.caps.iter().map(|&c| c as u64).sum()
    }
}

/// Minimum spare heap bytes for edits (64 KiB). `VXR_HEAP_HEADROOM` overrides it natively, to
/// exercise the heap-full rebuild in tests.
fn min_heap_headroom() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(v) = std::env::var("VXR_HEAP_HEADROOM").ok().and_then(|v| v.parse().ok()) {
        return v;
    }
    64 << 10
}

fn brick_prefix(map: &BrickMap, b: usize) -> u32 {
    let m = &map.masks[b * MASK_WORDS..(b + 1) * MASK_WORDS];
    let c = |n: usize| m[..n].iter().map(|w| w.count_ones()).sum::<u32>();
    c(4) | c(8) << 9 | c(12) << 18
}

fn brick_popcount(map: &BrickMap, b: usize) -> u32 {
    map.masks[b * MASK_WORDS..(b + 1) * MASK_WORDS].iter().map(|w| w.count_ones()).sum()
}

/// Palette bytes of the occupied voxels of brick `b`, in local index order, padded to 4.
fn pack_brick(map: &BrickMap, b: usize, out: &mut Vec<u8>) {
    out.clear();
    let m = &map.masks[b * MASK_WORDS..(b + 1) * MASK_WORDS];
    let p = &map.pal[b * BRICK_VOXELS..(b + 1) * BRICK_VOXELS];
    if m.iter().all(|&w| w == u32::MAX) {
        out.extend_from_slice(p);
        return;
    }
    for (wi, &w) in m.iter().enumerate() {
        let mut bits = w;
        while bits != 0 {
            let t = bits.trailing_zeros() as usize;
            out.push(p[wi * 32 + t]);
            bits &= bits - 1;
        }
    }
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

/// Heap and edit counters for the results.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct HeapStats {
    /// Bytes below the bump pointer (live blocks, free blocks and page-end gaps).
    pub top: u64,
    /// Bytes in blocks owned by live bricks.
    pub live_blocks: u64,
    /// Bytes allocated for the heap on the GPU.
    pub capacity: u64,
}

/// The brick map on the GPU. Bind group 1 of any pass that traces rays.
pub struct GpuBrickMap {
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    info: wgpu::Buffer,
    palette: wgpu::Buffer,
    grid: wgpu::Buffer,
    masks: wgpu::Buffer,
    coarse: wgpu::Buffer,
    pages: Vec<wgpu::Buffer>,
    /// Brick slots allocated on the GPU (live bricks + headroom).
    pub capacity: u32,
    coarse_cpu: Vec<u32>,
    sdims: [u32; 3],
    bdims: [u32; 3],
    lo: [u32; 3],
    hi: [u32; 3],
    /// Bytes allocated on the GPU for scene data, headroom included.
    pub allocated_bytes: u64,
    map_layout: MapLayout,
    heap: Option<CompactHeap>,
    scratch: Vec<u8>,
}

impl GpuBrickMap {
    /// Dense layout (512 palette bytes per brick).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, map: &BrickMap, palette: &[u32; 256]) -> Result<Self, String> {
        Self::new_with_layout(device, queue, layout, map, palette, MapLayout::Dense)
    }

    pub fn new_with_layout(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        map: &BrickMap,
        palette: &[u32; 256],
        map_layout: MapLayout,
    ) -> Result<Self, String> {
        let slots = map.brick_slots() as u32;
        let headroom = (slots / 8).max(512);
        let max_slots = if map_layout == MapLayout::Dense { PAGE_BRICKS * MAX_PAGES } else { u32::MAX / 2 };
        let capacity = (slots + headroom).min(max_slots);
        if slots > capacity {
            return Err(format!("{slots} bricks exceed the {} brick limit of four 128 MiB pages", PAGE_BRICKS * MAX_PAGES));
        }
        let bdims = map.dims;
        let sdims = [bdims[0].div_ceil(4), bdims[1].div_ceil(4), bdims[2].div_ceil(4)];
        let mut coarse_cpu = vec![0u32; (sdims[0] * sdims[1] * sdims[2] * 2) as usize];
        for bz in 0..bdims[2] {
            for by in 0..bdims[1] {
                for bx in 0..bdims[0] {
                    if map.grid[map.cell_index(bx, by, bz)] != EMPTY {
                        let (si, bit) = coarse_bit(sdims, bx, by, bz);
                        coarse_cpu[si * 2 + (bit >> 5) as usize] |= 1 << (bit & 31);
                    }
                }
            }
        }
        let (lo, hi) = map.occupied_bounds().unwrap_or(([0; 3], [0; 3]));

        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let mk = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage: storage, mapped_at_creation: false })
        };
        let grid = mk("grid", map.grid.len() as u64 * 4);
        queue.write_buffer(&grid, 0, bytemuck::cast_slice(&map.grid));
        // Compacted: the per-brick heap offsets live after the masks in the same buffer.
        let offsets_words = if map_layout == MapLayout::Compact { capacity as u64 * 2 } else { 0 };
        let masks = mk("masks", capacity as u64 * MASK_WORDS as u64 * 4 + offsets_words * 4);
        queue.write_buffer(&masks, 0, bytemuck::cast_slice(&map.masks));
        let coarse = mk("coarse", coarse_cpu.len() as u64 * 4);
        queue.write_buffer(&coarse, 0, bytemuck::cast_slice(&coarse_cpu));
        let mut pages = Vec::new();
        let mut allocated = grid.size() + masks.size() + coarse.size();
        let mut heap = None;
        match map_layout {
            MapLayout::Dense => {
                for p in 0..MAX_PAGES {
                    let first = p * PAGE_BRICKS;
                    if first < capacity || p == 0 {
                        let n = (capacity - first).min(PAGE_BRICKS);
                        let buf = mk("palette page", n as u64 * BRICK_VOXELS as u64);
                        let lo_b = first as usize * BRICK_VOXELS;
                        let hi_b = ((first + n) as usize * BRICK_VOXELS).min(map.pal.len());
                        if lo_b < hi_b {
                            queue.write_buffer(&buf, 0, &map.pal[lo_b..hi_b]);
                        }
                        allocated += buf.size();
                        pages.push(buf);
                    } else {
                        pages.push(mk("palette page (unused)", 16));
                    }
                }
            }
            MapLayout::Compact => {
                let mut h = CompactHeap {
                    offsets: (0..capacity as usize * 2).map(|i| if i % 2 == 0 { NO_BLOCK } else { 0 }).collect(),
                    caps: vec![0; capacity as usize],
                    free: vec![Vec::new(); (BRICK_VOXELS as u32 / BLOCK + 1) as usize],
                    top: 0,
                    capacity: u64::MAX,
                };
                let mut bytes: Vec<u8> = Vec::new();
                let mut tmp = Vec::with_capacity(BRICK_VOXELS);
                for b in 0..slots as usize {
                    let n = brick_popcount(map, b);
                    if n == 0 {
                        continue;
                    }
                    let cap = n.div_ceil(BLOCK) * BLOCK;
                    let off = h.alloc(cap).unwrap();
                    h.offsets[b * 2] = off;
                    h.offsets[b * 2 + 1] = brick_prefix(map, b);
                    h.caps[b] = cap;
                    pack_brick(map, b, &mut tmp);
                    let end = off as usize + cap as usize;
                    if bytes.len() < end {
                        bytes.resize(end, 0);
                    }
                    bytes[off as usize..off as usize + tmp.len()].copy_from_slice(&tmp);
                }
                // Headroom for edits that grow bricks: an eighth of the heap, at least 64 KiB.
                h.capacity = (h.top + (h.top / 8).max(min_heap_headroom())).div_ceil(4) * 4;
                let n_pages = h.capacity.div_ceil(HEAP_PAGE);
                if n_pages > MAX_PAGES as u64 {
                    return Err(format!("compacted heap of {} bytes needs more than four 128 MiB pages", h.capacity));
                }
                for p in 0..MAX_PAGES as u64 {
                    if p < n_pages || p == 0 {
                        let size = (h.capacity - p * HEAP_PAGE).min(HEAP_PAGE);
                        let buf = mk("heap page", size);
                        let lo_b = (p * HEAP_PAGE) as usize;
                        let hi_b = (lo_b + size as usize).min(bytes.len());
                        if lo_b < hi_b {
                            queue.write_buffer(&buf, 0, &bytes[lo_b..hi_b]);
                        }
                        allocated += buf.size();
                        pages.push(buf);
                    } else {
                        pages.push(mk("heap page (unused)", 16));
                    }
                }
                queue.write_buffer(&masks, capacity as u64 * MASK_WORDS as u64 * 4, bytemuck::cast_slice(&h.offsets));
                heap = Some(h);
            }
        }
        let info = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map info"),
            size: std::mem::size_of::<MapInfo>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let palette_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("palette"),
            contents: bytemuck::cast_slice(&palette_linear(palette)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let entries: Vec<wgpu::BindGroupEntry> = [&info, &palette_buf, &grid, &masks, &coarse, &pages[0], &pages[1], &pages[2], &pages[3]]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
            .collect();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("brickmap"), layout, entries: &entries });
        let me = Self {
            layout: layout.clone(),
            bind_group,
            info,
            palette: palette_buf,
            grid,
            masks,
            coarse,
            pages,
            capacity,
            coarse_cpu,
            sdims,
            bdims,
            lo,
            hi,
            allocated_bytes: allocated,
            map_layout,
            heap,
            scratch: Vec::with_capacity(BRICK_VOXELS),
        };
        me.write_info(queue);
        Ok(me)
    }

    /// Bytes of the coarse occupancy level.
    pub fn coarse_bytes(&self) -> u64 {
        self.coarse_cpu.len() as u64 * 4
    }

    pub fn map_layout(&self) -> MapLayout {
        self.map_layout
    }

    pub fn heap_stats(&self) -> Option<HeapStats> {
        self.heap.as_ref().map(|h| HeapStats { top: h.top, live_blocks: h.live_block_bytes(), capacity: h.capacity })
    }

    fn write_info(&self, queue: &wgpu::Queue) {
        let compact = self.map_layout == MapLayout::Compact;
        let info = MapInfo {
            bdims: [self.bdims[0], self.bdims[1], self.bdims[2], compact as u32],
            sdims: [self.sdims[0], self.sdims[1], self.sdims[2], if compact { self.capacity * MASK_WORDS as u32 } else { 0 }],
            lo: [self.lo[0] as f32, self.lo[1] as f32, self.lo[2] as f32, 0.0],
            hi: [self.hi[0] as f32, self.hi[1] as f32, self.hi[2] as f32, 0.0],
        };
        queue.write_buffer(&self.info, 0, bytemuck::bytes_of(&info));
    }

    /// Recolor: rewrite the 4 KB palette. No voxel data changes.
    pub fn set_palette(&self, queue: &wgpu::Queue, palette: &[u32; 256]) {
        queue.write_buffer(&self.palette, 0, bytemuck::cast_slice(&palette_linear(palette)));
    }

    /// Exact bytes of scene data in use (no pool headroom). Dense: grid + coarse + masks and
    /// 512 palette bytes per brick slot. Compacted: grid + coarse + masks, offset and prefix
    /// words per live brick + the 16-byte heap blocks of live bricks.
    pub fn used_bytes(&self, map: &BrickMap) -> u64 {
        let base = map.grid.len() as u64 * 4 + self.coarse_cpu.len() as u64 * 4;
        match &self.heap {
            None => map.gpu_bytes() + self.coarse_cpu.len() as u64 * 4,
            Some(h) => base + map.live_bricks() as u64 * (MASK_WORDS as u64 * 4 + 8) + h.live_block_bytes(),
        }
    }

    /// Upload only the bricks and grid cells an edit touched. If the pool is full it is
    /// rebuilt larger (and the bind group replaced).
    pub fn upload_dirty(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, map: &mut BrickMap, palette: &[u32; 256]) -> Result<UploadStats, String> {
        let (mut bricks, mut cells) = map.take_dirty();
        let mut st = UploadStats { bricks: bricks.len() as u32, cells: cells.len() as u32, ..Default::default() };
        if map.brick_slots() as u32 > self.capacity {
            *self = Self::new_with_layout(device, queue, &self.layout.clone(), map, palette, self.map_layout)?;
            st.rebuilt = true;
            return Ok(st);
        }
        bricks.sort_unstable();
        cells.sort_unstable();
        cells.dedup();
        // Masks, merged into runs of consecutive ids.
        for run in runs(&bricks) {
            let (a, n) = (run.0 as usize, run.1 as usize);
            queue.write_buffer(&self.masks, (a * MASK_WORDS * 4) as u64, bytemuck::cast_slice(&map.masks[a * MASK_WORDS..(a + n) * MASK_WORDS]));
            st.bytes += (n * MASK_WORDS * 4) as u64;
            st.writes += 1;
        }
        if self.map_layout == MapLayout::Compact {
            if self.upload_compact(queue, map, &bricks, &mut st).is_none() {
                // Heap full: repack everything into a bigger heap.
                *self = Self::new_with_layout(device, queue, &self.layout.clone(), map, palette, self.map_layout)?;
                st.rebuilt = true;
                return Ok(st);
            }
        } else {
            for run in runs(&bricks) {
                let (a, n) = (run.0 as usize, run.1 as usize);
                // A run may cross a page boundary.
                let mut b = a as u32;
                let end = (a + n) as u32;
                while b < end {
                    let page = b >> PAGE_SHIFT;
                    let page_end = ((page + 1) << PAGE_SHIFT).min(end);
                    let off = ((b & (PAGE_BRICKS - 1)) as usize * BRICK_VOXELS) as u64;
                    let src = &map.pal[b as usize * BRICK_VOXELS..page_end as usize * BRICK_VOXELS];
                    queue.write_buffer(&self.pages[page as usize], off, src);
                    st.bytes += src.len() as u64;
                    st.writes += 1;
                    b = page_end;
                }
            }
        }
        // Grid cells and the coarse bits above them.
        let mut coarse_dirty = Vec::new();
        for run in runs(&cells) {
            let (a, n) = (run.0 as usize, run.1 as usize);
            queue.write_buffer(&self.grid, (a * 4) as u64, bytemuck::cast_slice(&map.grid[a..a + n]));
            st.bytes += (n * 4) as u64;
            st.writes += 1;
            for ci in a..a + n {
                let bx = ci as u32 % self.bdims[0];
                let by = (ci as u32 / self.bdims[0]) % self.bdims[1];
                let bz = ci as u32 / (self.bdims[0] * self.bdims[1]);
                let (si, bit) = coarse_bit(self.sdims, bx, by, bz);
                let w = &mut self.coarse_cpu[si * 2 + (bit >> 5) as usize];
                if map.grid[ci] != EMPTY {
                    *w |= 1 << (bit & 31);
                    for (a, p) in [bx, by, bz].into_iter().enumerate() {
                        self.lo[a] = self.lo[a].min(p * 8);
                        self.hi[a] = self.hi[a].max(p * 8 + 8);
                    }
                } else {
                    *w &= !(1 << (bit & 31));
                }
                coarse_dirty.push(si as u32);
            }
        }
        coarse_dirty.sort_unstable();
        coarse_dirty.dedup();
        for run in runs(&coarse_dirty) {
            let (a, n) = (run.0 as usize, run.1 as usize);
            queue.write_buffer(&self.coarse, (a * 8) as u64, bytemuck::cast_slice(&self.coarse_cpu[a * 2..(a + n) * 2]));
            st.bytes += (n * 8) as u64;
            st.writes += 1;
        }
        st.coarse_cells = coarse_dirty.len() as u32;
        if !cells.is_empty() {
            self.write_info(queue);
        }
        Ok(st)
    }

    /// Re-pack the touched bricks into the heap, moving any whose block no longer fits.
    /// Returns None when the heap is full.
    fn upload_compact(&mut self, queue: &wgpu::Queue, map: &BrickMap, bricks: &[u32], st: &mut UploadStats) -> Option<()> {
        let h = self.heap.as_mut().unwrap();
        for &b in bricks {
            let bi = b as usize;
            let n = brick_popcount(map, bi);
            if n == 0 {
                h.release(bi);
                continue;
            }
            let need = n.div_ceil(BLOCK) * BLOCK;
            if need > h.caps[bi] || need * 2 < h.caps[bi] {
                h.release(bi);
                let off = h.alloc(need)?;
                h.offsets[bi * 2] = off;
                h.caps[bi] = need;
                st.reallocs += 1;
            }
            h.offsets[bi * 2 + 1] = brick_prefix(map, bi);
            pack_brick(map, bi, &mut self.scratch);
            let off = h.offsets[bi * 2] as u64;
            let page = (off / HEAP_PAGE) as usize;
            queue.write_buffer(&self.pages[page], off % HEAP_PAGE, &self.scratch);
            st.bytes += self.scratch.len() as u64;
            st.writes += 1;
        }
        // Offset and prefix words of every touched brick, in runs.
        let base = self.capacity as u64 * MASK_WORDS as u64 * 4;
        for run in runs(bricks) {
            let (a, n) = (run.0 as usize, run.1 as usize);
            queue.write_buffer(&self.masks, base + a as u64 * 8, bytemuck::cast_slice(&h.offsets[a * 2..(a + n) * 2]));
            st.bytes += n as u64 * 8;
            st.writes += 1;
        }
        Some(())
    }
}

fn coarse_bit(sdims: [u32; 3], bx: u32, by: u32, bz: u32) -> (usize, u32) {
    let (sx, sy, sz) = (bx / 4, by / 4, bz / 4);
    let si = (sx + sdims[0] * (sy + sdims[1] * sz)) as usize;
    let bit = (bx % 4) + (by % 4) * 4 + (bz % 4) * 16;
    (si, bit)
}

/// Runs of consecutive values in a sorted list: (start, length).
fn runs(v: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &x in v {
        match out.last_mut() {
            Some((s, n)) if *s + *n == x => *n += 1,
            _ => out.push((x, 1)),
        }
    }
    out
}

fn palette_linear(p: &[u32; 256]) -> Vec<[f32; 4]> {
    p.iter()
        .map(|&c| {
            let b = c.to_le_bytes();
            [srgb_to_linear(b[0]), srgb_to_linear(b[1]), srgb_to_linear(b[2]), b[3] as f32 / 255.0]
        })
        .collect()
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    inv_vp: [[f32; 4]; 4],
    eye: [f32; 4],
    size: [f32; 4],
    sun: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    /// Also write the depth and info G-buffer (for the path tracer).
    pub gbuffer: bool,
    /// 3 = coarse + brick + voxel DDA, 2 = brick + voxel only.
    pub levels: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { gbuffer: false, levels: 3 }
    }
}

/// The editor-view ray marcher plus a blit to any target format.
pub struct Renderer {
    pub width: u32,
    pub height: u32,
    pub options: RenderOptions,
    pub heatmap: bool,
    /// Test mode: also trace one secondary ray per pixel from the primary hit.
    pub secondary: bool,
    cam: wgpu::Buffer,
    out_layout: wgpu::BindGroupLayout,
    out_group: wgpu::BindGroup,
    pub color: wgpu::Texture,
    pub depth: wgpu::Texture,
    pub info: wgpu::Texture,
    march: wgpu::ComputePipeline,
    blit: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    blit_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, map_layout: &wgpu::BindGroupLayout, target_format: wgpu::TextureFormat, width: u32, height: u32, options: RenderOptions) -> Self {
        let out_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("march out"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                storage_tex(1, wgpu::TextureFormat::Rgba8Unorm),
                storage_tex(2, wgpu::TextureFormat::R32Float),
                storage_tex(3, wgpu::TextureFormat::R32Uint),
            ],
        });
        let cam = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let src = format!("{BRICKMAP_WGSL}\n{MARCH_WGSL}");
        let module = march_module(device, src);
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("march"),
            bind_group_layouts: &[Some(&out_layout), Some(map_layout)],
            immediate_size: 0,
        });
        let wg = workgroup();
        let constants = [
            ("WRITE_GBUFFER", if options.gbuffer { 1.0 } else { 0.0 }),
            ("LEVELS", options.levels as f64),
            ("WG_X", wg.0 as f64),
            ("WG_Y", wg.1 as f64),
        ];
        let march = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("march"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, zero_initialize_workgroup_memory: false },
            cache: None,
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()) });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_pl),
            vertex: wgpu::VertexState { module: &blit_module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let (color, depth, info, out_group, blit_group) = Self::targets(device, &out_layout, &blit_layout, &sampler, &cam, width, height, options.gbuffer);
        Self { width, height, options, heatmap: false, secondary: false, cam, out_layout, out_group, color, depth, info, march, blit, blit_layout, blit_group, sampler }
    }

    #[allow(clippy::too_many_arguments)]
    fn targets(
        device: &wgpu::Device,
        out_layout: &wgpu::BindGroupLayout,
        blit_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        cam: &wgpu::Buffer,
        w: u32,
        h: u32,
        gbuffer: bool,
    ) -> (wgpu::Texture, wgpu::Texture, wgpu::Texture, wgpu::BindGroup, wgpu::BindGroup) {
        let tex = |label, format, w: u32, h: u32, extra: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::STORAGE_BINDING | extra,
                view_formats: &[],
            })
        };
        let (gw, gh) = if gbuffer { (w, h) } else { (1, 1) };
        let color = tex("color", wgpu::TextureFormat::Rgba8Unorm, w, h, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let depth = tex("depth", wgpu::TextureFormat::R32Float, gw, gh, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let info = tex("info", wgpu::TextureFormat::R32Uint, gw, gh, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let v = |t: &wgpu::Texture| t.create_view(&Default::default());
        let (cv, dv, iv) = (v(&color), v(&depth), v(&info));
        let out_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("march out"),
            layout: out_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cam.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&cv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&iv) },
            ],
        });
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&cv) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        (color, depth, info, out_group, blit_group)
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if (width, height) == (self.width, self.height) {
            return;
        }
        let (c, d, i, og, bg) = Self::targets(device, &self.out_layout, &self.blit_layout, &self.sampler, &self.cam, width, height, self.options.gbuffer);
        (self.color, self.depth, self.info, self.out_group, self.blit_group) = (c, d, i, og, bg);
        self.width = width;
        self.height = height;
    }

    pub fn set_camera(&self, queue: &wgpu::Queue, cam: &OrbitCamera) {
        let vp = cam.view_proj(self.width as f32 / self.height as f32);
        let eye = cam.eye();
        let sun = glam::Vec3::new(0.45, 0.8, 0.35).normalize();
        let u = CameraUniform {
            inv_vp: vp.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
            size: [self.width as f32, self.height as f32, 1.0 / self.width as f32, 1.0 / self.height as f32],
            sun: [sun.x, sun.y, sun.z, (self.heatmap as u32 | (self.secondary as u32) << 1) as f32],
        };
        queue.write_buffer(&self.cam, 0, bytemuck::bytes_of(&u));
    }

    /// Ray march into the renderer's color (and G-buffer) textures.
    pub fn march(&self, encoder: &mut wgpu::CommandEncoder, map: &GpuBrickMap, timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("march"), timestamp_writes: timestamps });
        pass.set_pipeline(&self.march);
        pass.set_bind_group(0, &self.out_group, &[]);
        pass.set_bind_group(1, &map.bind_group, &[]);
        let wg = workgroup();
        pass.dispatch_workgroups(self.width.div_ceil(wg.0), self.height.div_ceil(wg.1), 1);
    }

    /// Draw the color texture onto `view` (window, canvas or offscreen), scaled to fit.
    pub fn blit(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &self.blit_group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn color_view(&self) -> wgpu::TextureView {
        self.color.create_view(&Default::default())
    }
    pub fn depth_view(&self) -> wgpu::TextureView {
        self.depth.create_view(&Default::default())
    }
    pub fn info_view(&self) -> wgpu::TextureView {
        self.info.create_view(&Default::default())
    }

    /// Read the color texture back as tightly packed RGBA8 rows.
    pub async fn read_color(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let row = self.width * 4;
        let padded = row.div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded as u64 * self.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(self.height) },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        let data = map_read(device, &buf).await;
        let mut out = Vec::with_capacity((row * self.height) as usize);
        for y in 0..self.height as usize {
            out.extend_from_slice(&data[y * padded as usize..y * padded as usize + row as usize]);
        }
        out
    }
}

/// The march shader. Natively, wgpu's runtime checks (buffer bounds checks and the counter
/// naga adds to every loop) are off unless `VXR_CHECKED=1`: the shader only indexes data the
/// engine wrote, and every loop is bounded by MAX_STEPS or the 8^3 brick. Browsers always
/// apply their own checks.
fn march_module(device: &wgpu::Device, src: String) -> wgpu::ShaderModule {
    let desc = wgpu::ShaderModuleDescriptor { label: Some("march"), source: wgpu::ShaderSource::Wgsl(src.into()) };
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var("VXR_CHECKED").is_err() {
        // SAFETY: see above; indices come from the engine's own grid, masks and pages.
        return unsafe { device.create_shader_module_trusted(desc, wgpu::ShaderRuntimeChecks::unchecked()) };
    }
    device.create_shader_module(desc)
}

/// March workgroup shape. `VXR_WG=16x8` overrides it for tuning runs (native only).
fn workgroup() -> (u32, u32) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(v) = std::env::var("VXR_WG") {
        if let Some((x, y)) = v.split_once('x') {
            return (x.parse().unwrap_or(8), y.parse().unwrap_or(8));
        }
    }
    (8, 8)
}

fn storage_tex(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format, view_dimension: wgpu::TextureViewDimension::D2 },
        count: None,
    }
}

/// Timestamp pairs (begin of march, end of blit) for up to `cap` frames.
pub struct GpuTimer {
    pub set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    pub cap: u32,
    period_ns: f32,
}

impl GpuTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, cap: u32) -> Self {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("frames"), ty: wgpu::QueryType::Timestamp, count: cap * 2 });
        let size = cap as u64 * 16;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ts resolve"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ts read"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self { set, resolve, read, cap, period_ns: queue.get_timestamp_period() }
    }

    pub fn begin(&self, i: u32) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites { query_set: &self.set, beginning_of_pass_write_index: Some(i * 2), end_of_pass_write_index: None }
    }
    pub fn end_compute(&self, i: u32) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites { query_set: &self.set, beginning_of_pass_write_index: None, end_of_pass_write_index: Some(i * 2 + 1) }
    }
    pub fn end(&self, i: u32) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites { query_set: &self.set, beginning_of_pass_write_index: None, end_of_pass_write_index: Some(i * 2 + 1) }
    }

    /// Milliseconds per pair for the first `n` pairs.
    pub async fn read_ms(&self, device: &wgpu::Device, queue: &wgpu::Queue, n: u32) -> Vec<f64> {
        let mut enc = device.create_command_encoder(&Default::default());
        enc.resolve_query_set(&self.set, 0..n * 2, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, Some(n as u64 * 16));
        queue.submit([enc.finish()]);
        let data = map_read(device, &self.read).await;
        let ts: &[u64] = bytemuck::cast_slice(&data[..n as usize * 16]);
        ts.chunks_exact(2).map(|p| p[1].wrapping_sub(p[0]) as f64 * self.period_ns as f64 / 1e6).collect()
    }
}

pub type SharedLayout = Arc<wgpu::BindGroupLayout>;
