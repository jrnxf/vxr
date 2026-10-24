//! Compare our reader with the `dot_vox` crate on every .vox file under the given paths:
//! voxels, palette, scene graph, layers, and world transforms. dot_vox does not compute
//! world transforms itself, so we flatten its graph two ways: with the composition rule
//! we use (and ogt_vox uses), and with the rule in dot_vox's own traverse_graph example.
//!
//! cargo run --release --example crosscheck_dotvox -- <file|dir>...

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dot_vox::SceneNode;
use vox::scene::{Affine, IDENTITY_R, Node, Rule, mat_mul, rotation_from_byte};
use vox::{Dict, Document};

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut es: Vec<_> = std::fs::read_dir(p).unwrap().flatten().map(|e| e.path()).collect();
        es.sort();
        for e in es {
            collect(&e, out);
        }
    } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vox")) {
        out.push(p.to_path_buf());
    }
}

type Map = BTreeMap<String, String>;

/// Our dict as a map (the last duplicate key wins, as in a HashMap insert).
fn as_map(d: &Dict) -> Map {
    d.0.iter()
        .map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned()))
        .collect()
}

fn bt<'a>(d: impl IntoIterator<Item = (&'a String, &'a String)>) -> Map {
    d.into_iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn local(d: &Map) -> Affine {
    let mut a = Affine::IDENTITY;
    if let Some(r) = d.get("_r").and_then(|v| v.trim().parse::<u8>().ok()).and_then(rotation_from_byte) {
        a.r = r;
    }
    if let Some(t) = d.get("_t") {
        let v: Vec<i32> = t.split_ascii_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() == 3 {
            a.t = [v[0], v[1], v[2]];
        }
    }
    a
}

/// Flatten dot_vox's graph (node id = position in `scenes`, as its example assumes).
fn flatten(dv: &dot_vox::DotVoxData, rule: Rule) -> Vec<(u32, Affine)> {
    let mut out = Vec::new();
    if dv.scenes.is_empty() {
        return out;
    }
    let mut stack = vec![(0u32, Affine::IDENTITY, 0usize)];
    while let Some((id, world, depth)) = stack.pop() {
        if depth > 256 || out.len() > 1 << 20 {
            continue;
        }
        let Some(n) = dv.scenes.get(id as usize) else { continue };
        match n {
            SceneNode::Transform { frames, child, .. } => {
                let f = frames.first().map(|f| bt(&f.attributes)).unwrap_or_default();
                let l = local(&f);
                let w = match rule {
                    Rule::Compose => world.then(&l),
                    Rule::DotVoxExample => {
                        let r = if f.contains_key("_r") { mat_mul(&world.r, &l.r) } else { IDENTITY_R };
                        Affine { r, t: [world.t[0] + l.t[0], world.t[1] + l.t[1], world.t[2] + l.t[2]] }
                    }
                };
                stack.push((*child, w, depth + 1));
            }
            SceneNode::Group { children, .. } => {
                for c in children.iter().rev() {
                    stack.push((*c, world, depth + 1));
                }
            }
            SceneNode::Shape { models, .. } => {
                if let Some(m) = models.first() {
                    out.push((m.model_id, world));
                }
            }
        }
    }
    out
}

fn main() {
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        collect(Path::new(&a), &mut files);
    }
    let mut tally: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut notes: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut check = |name: &'static str, ok: bool, f: &Path, why: String| {
        let t = tally.entry(name).or_default();
        if ok {
            t.0 += 1;
        } else {
            t.1 += 1;
            notes.entry(name).or_default().push(format!("{}: {why}", f.display()));
        }
    };
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let ours = Document::read(&bytes).and_then(|d| d.scene());
        let theirs = dot_vox::load_bytes(&bytes);
        let (s, dv) = match (ours, theirs) {
            (Ok(s), Ok(dv)) => (s, dv),
            (Err(_), Err(_)) => {
                check("both reject", true, f, String::new());
                continue;
            }
            (a, b) => {
                check("both read", false, f, format!("ours ok {}, dot_vox ok {}", a.is_ok(), b.is_ok()));
                continue;
            }
        };
        // Voxels: dot_vox stores index - 1 (saturating), so compare that way.
        let mut vox_ok = s.models.len() == dv.models.len();
        let mut why = format!("{} vs {} models", s.models.len(), dv.models.len());
        for (m, d) in s.models.iter().zip(&dv.models) {
            let same_size = [d.size.x as i32, d.size.y as i32, d.size.z as i32] == m.size;
            let same_vox = m.voxels.len() == d.voxels.len()
                && m.voxels
                    .iter()
                    .zip(&d.voxels)
                    .all(|(a, b)| [a[0], a[1], a[2], a[3].saturating_sub(1)] == [b.x, b.y, b.z, b.i]);
            if !(same_size && same_vox) {
                vox_ok = false;
                why = "a model's size or voxels differ".into();
            }
        }
        check("voxels", vox_ok, f, why);
        // Entries 0..255 are voxel indices 1..255. Entry 255 is never used by a voxel;
        // the two default palettes disagree on its alpha, so it is tallied on its own.
        let used = |v: &[dot_vox::Color]| v.iter().take(255).map(|c| [c.r, c.g, c.b, c.a]).collect::<Vec<_>>();
        let pal_ok = used(&dv.palette) == s.palette[..255];
        check(
            "palette entry 256 (unused)",
            dv.palette.get(255).map(|c| [c.r, c.g, c.b, c.a]) == Some(s.palette[255]),
            f,
            String::new(),
        );
        let first = dv.palette.iter().take(255).zip(&s.palette).position(|(c, p)| [c.r, c.g, c.b, c.a] != *p);
        let why = match first {
            Some(i) => format!(
                "{} entries; first difference at file entry {i}: ours {:?}, dot_vox {:?}",
                dv.palette.len(),
                s.palette[i],
                dv.palette[i]
            ),
            None => format!("{} entries", dv.palette.len()),
        };
        check("palette", pal_ok, f, why);
        // Scene graph, node by node in file order.
        let ours_nodes: Vec<&Node> = {
            let mut v: Vec<(usize, &Node)> = s.nodes.iter().map(|(id, n)| (s.node_chunks[id], n)).collect();
            v.sort_by_key(|x| x.0);
            v.into_iter().map(|x| x.1).collect()
        };
        let mut graph_ok = ours_nodes.len() == dv.scenes.len();
        for (a, b) in ours_nodes.iter().zip(&dv.scenes) {
            let same = match (a, b) {
                (Node::Transform(t), SceneNode::Transform { attributes, frames, child, layer_id }) => {
                    as_map(&t.attrs) == bt(attributes)
                        && t.child as u32 == *child
                        && t.layer as u32 == *layer_id
                        && t.frames.len() == frames.len()
                        && t.frames.iter().zip(frames).all(|(x, y)| as_map(x) == bt(&y.attributes))
                }
                (Node::Group(g), SceneNode::Group { attributes, children }) => {
                    as_map(&g.attrs) == bt(attributes)
                        && g.children.iter().map(|&c| c as u32).eq(children.iter().copied())
                }
                (Node::Shape(sh), SceneNode::Shape { attributes, models }) => {
                    as_map(&sh.attrs) == bt(attributes)
                        && sh.models.len() == models.len()
                        && sh
                            .models
                            .iter()
                            .zip(models)
                            .all(|(x, y)| x.0 as u32 == y.model_id && as_map(&x.1) == bt(&y.attributes))
                }
                _ => false,
            };
            graph_ok &= same;
        }
        check("scene graph", graph_ok, f, format!("{} vs {} nodes", ours_nodes.len(), dv.scenes.len()));
        let layers_ok = s.layers.len() == dv.layers.len()
            && s.layers.iter().zip(&dv.layers).all(|(a, b)| as_map(&a.attrs) == bt(&b.attributes));
        check("layers", layers_ok, f, format!("{} vs {}", s.layers.len(), dv.layers.len()));
        if s.nodes.is_empty() {
            continue;
        }
        let (inst, _) = s.instances(Rule::Compose);
        let ours_w: Vec<(u32, Affine)> = inst.iter().map(|i| (i.model as u32, i.world)).collect();
        let base_keys =
            s.nodes.values().any(|n| matches!(n, Node::Transform(t) if t.attrs.has("_frm_t") || t.attrs.has("_frm_r")));
        let a = flatten(&dv, Rule::Compose);
        check(
            "world (compose rule)",
            a == ours_w,
            f,
            if base_keys {
                "file has _frm_t/_frm_r, which we (and ogt_vox) use and dot_vox ignores".into()
            } else {
                format!("{} vs {} instances", ours_w.len(), a.len())
            },
        );
        let b = flatten(&dv, Rule::DotVoxExample);
        let n = b.iter().zip(&ours_w).filter(|(x, y)| x != y).count();
        check(
            "world (dot_vox example rule)",
            b == ours_w,
            f,
            format!("{n} of {} instances placed differently", ours_w.len()),
        );
    }
    println!("dot_vox cross-check over {} files", files.len());
    for (k, (ok, bad)) in &tally {
        println!("  {k:<30} same {ok:>4}  differ {bad:>4}");
    }
    for (k, v) in &notes {
        println!("\n{k}:");
        for n in v.iter().take(40) {
            println!("  {n}");
        }
        if v.len() > 40 {
            println!("  ... {} more", v.len() - 40);
        }
    }
}
