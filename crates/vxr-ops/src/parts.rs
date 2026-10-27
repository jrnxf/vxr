//! Named parts, so a model finds "the chimney" by reading a list instead of probing with
//! `select`: every connected run of one colour in the scene as opened (through faces and edges, with
//! shades of a colour taken as one: painted noise on grass or a two-tone roof stays one
//! part), named by its colour and shape ("brown column") and the part it rests on, plus the
//! parts the model's own edits named. Parts keep their map cells; an edit on a part takes those cells
//! that still hold a voxel.

use std::collections::HashMap;

use vxr_core::Scene;

use crate::args::Cell;
use crate::text::{clean, color_distance, color_name};

/// Above this many voxels the scene is not split into parts (a flood fill per voxel).
pub const MAX_VOXELS: usize = 2_000_000;
/// Neighbours whose colours are closer than this (CIE76) are one part: shades of grass or of
/// a roof, not a roof and its wooden wall.
const SHADE: f32 = 22.0;

/// The 18 cells that share a face or an edge with a cell: the steps of a stair or a gable
/// roof touch only along edges and are still one part.
const NEIGHBOURS: [[i32; 3]; 18] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
    [1, 1, 0],
    [1, -1, 0],
    [-1, 1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [1, 0, -1],
    [-1, 0, 1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, 1, -1],
    [0, -1, 1],
    [0, -1, -1],
];

#[derive(Clone, Debug)]
pub struct Part {
    /// `p1`, `p2`, ...: the scene's parts by size, then the model's in the order it named them.
    pub id: String,
    pub name: String,
    /// The slots of a detected part, most used first; empty for a part the model named.
    pub slots: Vec<u8>,
    /// Map cells.
    pub cells: Vec<Cell>,
    /// Index of the part most of this one's bottom rests on.
    pub on: Option<usize>,
    /// Named by the model's edit, not found in the scene.
    pub named: bool,
}

impl Part {
    /// Whether every word of `query` is in the name, or `query` is the id.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q == self.id {
            return true;
        }
        let name = self.name.to_lowercase();
        let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        let mut asked = q.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).peekable();
        asked.peek().is_some()
            && asked.all(|a| words.iter().any(|w| w.starts_with(a) || (a.starts_with(*w) && w.len() > 2)))
    }
}

/// A shape word from the extent of a part: voxel, column, slab, strip, wall or blob.
fn shape(e: [i32; 3]) -> &'static str {
    let (w, h, d) = (e[0], e[1], e[2]);
    let (lo, hi) = (w.min(d), w.max(d));
    if w == 1 && h == 1 && d == 1 {
        "voxel"
    } else if h >= 2 * hi {
        "column"
    } else if h * 3 <= hi {
        if lo * 3 <= hi { "strip" } else { "slab" }
    } else if lo * 3 <= hi {
        "wall"
    } else {
        "blob"
    }
}

/// The scene's parts: each connected run of one colour and its shades (through faces and edges),
/// biggest first. Empty over [`MAX_VOXELS`].
pub fn detect(scene: &Scene) -> Vec<Part> {
    if scene.map.voxels as usize > MAX_VOXELS {
        return Vec::new();
    }
    let mut same = vec![false; 256 * 256];
    for a in 1..256 {
        for b in 1..256 {
            same[a * 256 + b] = a == b || color_distance(scene.palette[a], scene.palette[b]) < SHADE;
        }
    }
    let mut cells: HashMap<Cell, (u8, u32)> = HashMap::new();
    scene.for_each_voxel(|p, v| {
        cells.insert(p, (v, u32::MAX));
    });
    let mut keys: Vec<Cell> = cells.keys().copied().collect();
    keys.sort_unstable();
    let mut runs: Vec<Vec<Cell>> = Vec::new();
    for start in keys {
        if cells[&start].1 != u32::MAX {
            continue;
        }
        let id = runs.len() as u32;
        let mut run = Vec::new();
        let mut stack = vec![start];
        cells.get_mut(&start).unwrap().1 = id;
        while let Some(p) = stack.pop() {
            run.push(p);
            let v = cells[&p].0 as usize;
            for d in NEIGHBOURS {
                let q = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if let Some(c) = cells.get_mut(&q).filter(|c| c.1 == u32::MAX && same[v * 256 + c.0 as usize]) {
                    c.1 = id;
                    stack.push(q);
                }
            }
        }
        runs.push(run);
    }
    // Biggest first; ties by first cell, so ids are the same on every run.
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by(|&a, &b| runs[b].len().cmp(&runs[a].len()).then(runs[a].iter().min().cmp(&runs[b].iter().min())));
    let mut rank = vec![0usize; runs.len()];
    for (i, &r) in order.iter().enumerate() {
        rank[r] = i;
    }
    order
        .iter()
        .map(|&r| {
            let run = &runs[r];
            let (lo, hi) = vxr_core::select::bounds(run.iter().copied()).unwrap();
            let extent: [i32; 3] = std::array::from_fn(|a| hi[a] - lo[a] + 1);
            let mut count = [0usize; 256];
            // What its bottom cells rest on: the other part under most of them.
            let mut below: HashMap<usize, usize> = HashMap::new();
            for p in run {
                count[cells[p].0 as usize] += 1;
                if let Some(&(_, l)) = cells.get(&[p[0], p[1] - 1, p[2]])
                    && l as usize != r
                {
                    *below.entry(rank[l as usize]).or_default() += 1;
                }
            }
            let mut slots: Vec<u8> = (1..=255u8).filter(|&s| count[s as usize] > 0).collect();
            slots.sort_by_key(|&s| std::cmp::Reverse(count[s as usize]));
            let on = below.into_iter().max_by_key(|&(i, n)| (n, std::cmp::Reverse(i))).map(|(i, _)| i);
            Part {
                id: format!("p{}", rank[r] + 1),
                name: format!("{} {}", color_name(scene.palette[slots[0] as usize]), shape(extent)),
                slots,
                cells: run.clone(),
                on,
                named: false,
            }
        })
        .collect()
}

/// A part the model named, from the cells its edit filled.
pub fn named(id: usize, name: &str, cells: Vec<Cell>) -> Part {
    Part { id: format!("p{id}"), name: clean(name.as_bytes()), slots: Vec::new(), cells, on: None, named: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert_eq!(shape([1, 1, 1]), "voxel");
        assert_eq!(shape([2, 8, 2]), "column");
        assert_eq!(shape([20, 2, 18]), "slab");
        assert_eq!(shape([20, 2, 2]), "strip");
        assert_eq!(shape([12, 6, 1]), "wall");
        assert_eq!(shape([5, 5, 5]), "blob");
    }

    #[test]
    fn detect_runs_and_rests() {
        let mut s = Scene::new([16, 16, 16]);
        // A brown slab with a red column on it, and a lone red voxel apart.
        let mut cells = Vec::new();
        for x in 0..8 {
            for z in 0..8 {
                cells.push(([x, 0, z], 10));
            }
        }
        for y in 1..6 {
            cells.push(([3, y, 3], 20));
        }
        cells.push(([12, 0, 12], 20));
        s.edit(cells);
        s.palette[10] = [140, 85, 40, 255];
        s.palette[20] = [215, 35, 35, 255];
        let parts = detect(&s);
        assert_eq!(parts.len(), 3);
        assert_eq!((parts[0].id.as_str(), parts[0].name.as_str(), parts[0].cells.len()), ("p1", "brown slab", 64));
        assert_eq!((parts[1].name.as_str(), parts[1].cells.len(), parts[1].on), ("red column", 5, Some(0)));
        assert_eq!((parts[2].name.as_str(), parts[2].on), ("red voxel", None));
        assert!(parts[1].matches("red column") && parts[1].matches("p2") && parts[1].matches("Column"));
        assert!(!parts[1].matches("brown") && !parts[1].matches("") && !parts[1].matches("p1"));
    }
}
