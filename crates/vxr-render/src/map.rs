//! The brick map on the GPU, in the compacted layout: masks for every brick, palette bytes
//! only for occupied voxels. Full upload on creation, partial upload after edits.

use vxr_core::brickmap::{BRICK_VOXELS, BrickMap, EMPTY, MASK_WORDS, MAX_PAGES};
use wgpu::util::DeviceExt;

/// One heap page: 128 MiB, the default max storage binding.
const HEAP_PAGE: u64 = 1 << 27;
/// Heap allocation unit in bytes.
const BLOCK: u32 = 16;
const NO_BLOCK: u32 = u32::MAX;
/// Spare heap bytes kept for edits that grow bricks, at least.
const MIN_HEAP_HEADROOM: u64 = 64 << 10;

/// Bind group 1 layout of any pass that traces rays (see `brickmap.wgsl`).
pub fn brickmap_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
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
        entries: &[
            uniform(0),
            uniform(1),
            storage(2),
            storage(3),
            storage(4),
            storage(5),
            storage(6),
            storage(7),
            storage(8),
        ],
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

/// What an upload wrote.
#[derive(Clone, Copy, Debug, Default)]
pub struct UploadStats {
    pub bricks: u32,
    pub cells: u32,
    pub bytes: u64,
    pub writes: u32,
    /// The pool or heap was full, so everything was uploaded again into bigger buffers.
    pub rebuilt: bool,
    /// Bricks whose heap block had to move (grew past its block, or shrank to under half).
    pub reallocs: u32,
}

/// CPU bookkeeping for the heap: one block per live brick, free lists per block size, bump
/// allocation at the top. Blocks never cross a page.
struct Heap {
    /// Two words per brick slot: heap byte offset, then the set-bit counts of mask words
    /// 0..4, 0..8 and 0..12 (9 bits each), so the shader's rank needs at most 4 popcounts.
    offsets: Vec<u32>,
    caps: Vec<u32>,
    free: Vec<Vec<u32>>,
    top: u64,
    capacity: u64,
}

impl Heap {
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
    while !out.len().is_multiple_of(4) {
        out.push(0);
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

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn palette_linear(p: &[[u8; 4]; 256]) -> Vec<[f32; 4]> {
    p.iter().map(|c| [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3] as f32 / 255.0]).collect()
}

/// The brick map on the GPU. Bind group 1 of any pass that traces rays.
pub struct GpuMap {
    pub bind_group: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    info: wgpu::Buffer,
    palette: wgpu::Buffer,
    grid: wgpu::Buffer,
    masks: wgpu::Buffer,
    coarse: wgpu::Buffer,
    pages: Vec<wgpu::Buffer>,
    /// Brick slots allocated on the GPU (live bricks plus headroom).
    capacity: u32,
    coarse_cpu: Vec<u32>,
    sdims: [u32; 3],
    bdims: [u32; 3],
    lo: [u32; 3],
    hi: [u32; 3],
    /// Bytes allocated on the GPU for scene data, headroom included.
    pub allocated_bytes: u64,
    heap: Heap,
    scratch: Vec<u8>,
}

impl GpuMap {
    /// Upload the whole map. `palette` is RGBA (sRGB) by voxel index.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        map: &BrickMap,
        palette: &[[u8; 4]; 256],
    ) -> Result<Self, String> {
        let slots = map.brick_slots() as u32;
        let capacity = slots + (slots / 8).max(512);
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

        let mut heap = Heap {
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
            let off = heap.alloc(cap).expect("unbounded during the first pack");
            heap.offsets[b * 2] = off;
            heap.offsets[b * 2 + 1] = brick_prefix(map, b);
            heap.caps[b] = cap;
            pack_brick(map, b, &mut tmp);
            let end = off as usize + cap as usize;
            if bytes.len() < end {
                bytes.resize(end, 0);
            }
            bytes[off as usize..off as usize + tmp.len()].copy_from_slice(&tmp);
        }
        // Headroom for edits that grow bricks: an eighth of the heap, at least 64 KiB.
        heap.capacity = (heap.top + (heap.top / 8).max(MIN_HEAP_HEADROOM)).div_ceil(4) * 4;
        let n_pages = heap.capacity.div_ceil(HEAP_PAGE);
        if n_pages > MAX_PAGES as u64 {
            return Err(format!("the scene's {} heap bytes need more than four 128 MiB pages", heap.capacity));
        }

        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let mk = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage: storage,
                mapped_at_creation: false,
            })
        };
        let grid = mk("grid", map.grid.len() as u64 * 4);
        queue.write_buffer(&grid, 0, bytemuck::cast_slice(&map.grid));
        // The per-brick heap offsets live after the masks in the same buffer.
        let masks = mk("masks", capacity as u64 * MASK_WORDS as u64 * 4 + capacity as u64 * 8);
        if !map.masks.is_empty() {
            queue.write_buffer(&masks, 0, bytemuck::cast_slice(&map.masks));
        }
        queue.write_buffer(&masks, capacity as u64 * MASK_WORDS as u64 * 4, bytemuck::cast_slice(&heap.offsets));
        let coarse = mk("coarse", coarse_cpu.len() as u64 * 4);
        queue.write_buffer(&coarse, 0, bytemuck::cast_slice(&coarse_cpu));
        let mut allocated = grid.size() + masks.size() + coarse.size();
        let mut pages = Vec::new();
        for p in 0..MAX_PAGES as u64 {
            if p < n_pages || p == 0 {
                let size = (heap.capacity - p * HEAP_PAGE).min(HEAP_PAGE);
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
        let entries: Vec<wgpu::BindGroupEntry> =
            [&info, &palette_buf, &grid, &masks, &coarse, &pages[0], &pages[1], &pages[2], &pages[3]]
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
                .collect();
        let bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("brickmap"), layout, entries: &entries });
        let me = Self {
            bind_group,
            layout: layout.clone(),
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
            heap,
            scratch: Vec::with_capacity(BRICK_VOXELS),
        };
        me.write_info(queue);
        Ok(me)
    }

    fn write_info(&self, queue: &wgpu::Queue) {
        let info = MapInfo {
            bdims: [self.bdims[0], self.bdims[1], self.bdims[2], 0],
            sdims: [self.sdims[0], self.sdims[1], self.sdims[2], self.capacity * MASK_WORDS as u32],
            lo: [self.lo[0] as f32, self.lo[1] as f32, self.lo[2] as f32, 0.0],
            hi: [self.hi[0] as f32, self.hi[1] as f32, self.hi[2] as f32, 0.0],
        };
        queue.write_buffer(&self.info, 0, bytemuck::bytes_of(&info));
    }

    /// Recolor: rewrite the 4 KB palette. No voxel data changes.
    pub fn set_palette(&self, queue: &wgpu::Queue, palette: &[[u8; 4]; 256]) {
        queue.write_buffer(&self.palette, 0, bytemuck::cast_slice(&palette_linear(palette)));
    }

    /// Upload only the bricks and grid cells changed since the last upload. If the brick pool
    /// or the heap is full, everything is uploaded again into bigger buffers (and the bind
    /// group replaced). The map must have dirty tracking on.
    pub fn upload_dirty(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        map: &mut BrickMap,
        palette: &[[u8; 4]; 256],
    ) -> Result<UploadStats, String> {
        let (mut bricks, mut cells) = map.take_dirty();
        let mut st = UploadStats { bricks: bricks.len() as u32, cells: cells.len() as u32, ..Default::default() };
        if bricks.is_empty() && cells.is_empty() {
            return Ok(st);
        }
        if map.brick_slots() as u32 > self.capacity {
            *self = Self::new(device, queue, &self.layout.clone(), map, palette)?;
            st.rebuilt = true;
            return Ok(st);
        }
        bricks.sort_unstable();
        bricks.dedup();
        cells.sort_unstable();
        cells.dedup();
        // Masks, merged into runs of consecutive ids.
        for &(a, n) in &runs(&bricks) {
            let (a, n) = (a as usize, n as usize);
            let src = &map.masks[a * MASK_WORDS..(a + n) * MASK_WORDS];
            queue.write_buffer(&self.masks, (a * MASK_WORDS * 4) as u64, bytemuck::cast_slice(src));
            st.bytes += (n * MASK_WORDS * 4) as u64;
            st.writes += 1;
        }
        if self.upload_heap(queue, map, &bricks, &mut st).is_none() {
            *self = Self::new(device, queue, &self.layout.clone(), map, palette)?;
            st.rebuilt = true;
            return Ok(st);
        }
        // Grid cells and the coarse bits above them.
        let mut coarse_dirty = Vec::new();
        for &(a, n) in &runs(&cells) {
            let (a, n) = (a as usize, n as usize);
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
                    let was_empty = self.lo == [0; 3] && self.hi == [0; 3];
                    for (axis, p) in [bx, by, bz].into_iter().enumerate() {
                        self.lo[axis] = if was_empty { p * 8 } else { self.lo[axis].min(p * 8) };
                        self.hi[axis] = self.hi[axis].max(p * 8 + 8);
                    }
                } else {
                    *w &= !(1 << (bit & 31));
                }
                coarse_dirty.push(si as u32);
            }
        }
        coarse_dirty.sort_unstable();
        coarse_dirty.dedup();
        for &(a, n) in &runs(&coarse_dirty) {
            let (a, n) = (a as usize, n as usize);
            queue.write_buffer(
                &self.coarse,
                (a * 8) as u64,
                bytemuck::cast_slice(&self.coarse_cpu[a * 2..(a + n) * 2]),
            );
            st.bytes += (n * 8) as u64;
            st.writes += 1;
        }
        if !cells.is_empty() {
            self.write_info(queue);
        }
        Ok(st)
    }

    /// Re-pack the touched bricks into the heap, moving any whose block no longer fits.
    /// Returns None when the heap is full.
    fn upload_heap(&mut self, queue: &wgpu::Queue, map: &BrickMap, bricks: &[u32], st: &mut UploadStats) -> Option<()> {
        let h = &mut self.heap;
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
        for &(a, n) in &runs(bricks) {
            let (a, n) = (a as usize, n as usize);
            queue.write_buffer(&self.masks, base + a as u64 * 8, bytemuck::cast_slice(&h.offsets[a * 2..(a + n) * 2]));
            st.bytes += n as u64 * 8;
            st.writes += 1;
        }
        Some(())
    }
}
