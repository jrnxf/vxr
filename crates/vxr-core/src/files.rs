//! Model exports as files: OBJ with its `.mtl` and palette texture, PNG slices, and a
//! stored (not compressed) zip to hand several files to a browser as one download. And the
//! `.vox` save: bytes checked to read back as the scene, written so a failure mid-write
//! never leaves a part-written file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Scene;
use crate::export::{TEXTURE_W, obj, slices};

/// A file to write or download: its name and bytes.
pub type File = (String, Vec<u8>);

/// The OBJ export of what the view shows (the edited model, or every shown object in the
/// world), named `<stem>.obj`, `<stem>.mtl` and `<stem>.png`, and its quad count. The OBJ
/// names its `.mtl` and the `.mtl` its texture, and both split names at whitespace and end
/// them at `#`, so each run of those in `stem` becomes `-`.
pub fn obj_files(scene: &Scene, stem: &str) -> Result<(Vec<File>, usize), String> {
    let parts: Vec<&str> =
        stem.split(|c: char| c.is_whitespace() || c.is_control() || c == '#').filter(|p| !p.is_empty()).collect();
    let stem = if parts.is_empty() { "vxr".to_string() } else { parts.join("-") };
    let stem = stem.as_str();
    let mut voxels = BTreeMap::new();
    scene.for_each_voxel(|p, c| {
        voxels.insert(scene.to_world(p), c);
    });
    if voxels.is_empty() {
        return Err("the scene is empty".into());
    }
    let o = obj(&voxels, &scene.palette, stem);
    let files = vec![
        (format!("{stem}.obj"), o.obj.into_bytes()),
        (format!("{stem}.mtl"), o.mtl.into_bytes()),
        (format!("{stem}.png"), png(TEXTURE_W, 1, &o.texture)?),
    ];
    Ok((files, o.quads))
}

/// The largest PNG slices image, in pixels (256 MiB of RGBA, which a browser tab can hold).
const SLICES_MAX: u64 = 1 << 26;

/// The PNG slices of what the view shows (the edited model's bounds, or the world's), as
/// one PNG: every layer from the bottom up, side by side. Returns the PNG, its size and the
/// number of layers.
pub fn slices_png(scene: &Scene) -> Result<(Vec<u8>, [u32; 2], u32), String> {
    let size = scene.bounds().size;
    if scene.map.counts[1..].iter().all(|&n| n == 0) {
        return Err("the scene is empty".into());
    }
    let pixels = size.iter().map(|&n| n as u64).product::<u64>();
    if pixels > SLICES_MAX {
        return Err(format!("{}×{}×{} is too large for one image", size[0], size[1], size[2]));
    }
    let mut cells = Vec::new();
    scene.for_each_voxel(|p, c| cells.push((p, c)));
    let s = slices(size, cells, &scene.palette);
    Ok((png(s.width, s.height, &s.rgba)?, [s.width, s.height], s.layers))
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

/// The scene as `.vox` bytes, checked to read back as the same scene: every voxel, the
/// shown ones, the palette, and for an opened file the names, hidden flags and layers of
/// its objects and groups and its layers.
/// A save that would lose any of them is an error naming the first thing it would lose,
/// not a file.
pub fn vox_bytes(scene: &Scene) -> Result<Vec<u8>, String> {
    let bytes = scene.to_vox()?;
    reads_back_as(&bytes, scene)?;
    Ok(bytes)
}

fn reads_back_as(bytes: &[u8], scene: &Scene) -> Result<(), String> {
    let again = Scene::from_vox(bytes).map_err(|e| format!("the result does not read back: {e}"))?;
    for shown in [true, false] {
        let (want, got) = (scene.world_voxels(shown), again.world_voxels(shown));
        if want != got {
            let which = if shown { "shown voxels" } else { "voxels" };
            return Err(format!("the result reads back with different {which}: {}", voxel_change(&want, &got, scene)));
        }
    }
    if let Some(slot) = (1..256).find(|&i| scene.palette[i] != again.palette[i]) {
        return Err(format!("the result reads back with a different colour {slot}"));
    }
    if let Some(w) = scene.world() {
        let (want, got) = (outline(w), again.world().map(outline).unwrap_or_default());
        if want != got {
            return Err(format!("the result reads back with {}", outline_change(&want, &got)));
        }
    }
    Ok(())
}

/// The first cell that differs between `want` and `got`, what it would become, and the
/// scene's object there, if any.
fn voxel_change(want: &BTreeMap<[i32; 3], u8>, got: &BTreeMap<[i32; 3], u8>, scene: &Scene) -> String {
    let cell = want
        .iter()
        .find(|&(p, c)| got.get(p) != Some(c))
        .map(|(p, _)| *p)
        .or_else(|| got.keys().find(|&p| !want.contains_key(p)).copied())
        .unwrap_or_default();
    let say = |c: Option<&u8>| c.map_or("empty".to_string(), |c| format!("colour {c}"));
    let object =
        scene.world_now().and_then(|w| object_name_at(&w, cell)).map(|n| format!(" in \"{n}\"")).unwrap_or_default();
    format!("[{}, {}, {}]{object} is {}, not {}", cell[0], cell[1], cell[2], say(got.get(&cell)), say(want.get(&cell)))
}

/// The name of the object with a voxel at vxr world cell `q`, or of its nearest named
/// group (a block of a model over 256 is an unnamed object in the object's group).
fn object_name_at(w: &crate::World, q: [i32; 3]) -> Option<String> {
    let mut found = None;
    w.for_each_voxel(false, |i, p, _| {
        if p == q && found.is_none() {
            found = Some(i);
        }
    });
    let parents = w.tree.parents();
    let mut k = found;
    while let Some(i) = k {
        if let Some(name) = &w.tree.nodes[i].name {
            return Some(String::from_utf8_lossy(name).into_owned());
        }
        k = parents[i];
    }
    None
}

/// What a save must keep of a world besides its voxels: each node's name, hidden flag and
/// layer in tree order, and the layers. Not the tree's shape: a model over 256 on an axis is
/// saved as a group of blocks under the object's name, each block an unnamed shown node on
/// the object's layer, so nodes that say nothing their parent does not are left out.
type Outline = (Vec<(Option<Vec<u8>>, bool, i32)>, Vec<vxr_vox::LayerInfo>);

fn outline(w: &crate::World) -> Outline {
    let parents = w.tree.parents();
    let nodes = w
        .tree
        .order()
        .into_iter()
        .filter(|&i| {
            let n = &w.tree.nodes[i];
            n.name.is_some() || n.hidden || n.layer != parents[i].map_or(0, |p| w.tree.nodes[p].layer)
        })
        .map(|i| {
            let n = &w.tree.nodes[i];
            (n.name.clone(), n.hidden, n.layer)
        })
        .collect();
    (nodes, w.layers())
}

/// The first node or layer that differs between two outlines, as the one the scene has.
fn outline_change(want: &Outline, got: &Outline) -> String {
    let name = |n: &Option<Vec<u8>>| n.as_ref().map(|n| format!("\"{}\"", String::from_utf8_lossy(n)));
    if let Some(i) = (0..want.0.len().max(got.0.len())).find(|&i| want.0.get(i) != got.0.get(i)) {
        return match want.0.get(i) {
            Some((n, hidden, layer)) => {
                let node = name(n).unwrap_or_else(|| "an unnamed node".into());
                let flag = if *hidden { "hidden" } else { "shown" };
                format!("a different name, hidden flag or layer on {node} ({flag}, on layer {layer})")
            }
            None => "a node the scene does not have".into(),
        };
    }
    match (0..want.1.len().max(got.1.len())).find(|&i| want.1.get(i) != got.1.get(i)).and_then(|i| want.1.get(i)) {
        Some(l) => format!("a different name or hidden flag on layer {}", l.id),
        None => "a layer the scene does not have".into(),
    }
}

/// Replace `path` with `bytes` so that it holds either its old bytes or all of the new
/// ones, whatever happens mid-write: they go to a temporary file beside it, named for this
/// save alone so two programs saving the same file at once cannot touch each other's, are
/// flushed and read back, and only then is that file renamed over `path`, keeping its
/// permissions. A symbolic link at `path` is replaced, not followed. A read-only file is
/// refused, as a plain write would be. Being a new file, the result does not keep the old
/// one's extended attributes (Finder tags) or creation date, and other hard links to the
/// old file keep its old bytes.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let old = std::fs::symlink_metadata(path).ok().filter(|m| m.is_file());
    if old.as_ref().is_some_and(|m| m.permissions().readonly()) {
        return Err("the file is read-only".into());
    }
    static SAVES: AtomicU64 = AtomicU64::new(0);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}-{}.tmp", std::process::id(), SAVES.fetch_add(1, Ordering::Relaxed)));
    let tmp = PathBuf::from(tmp);
    write_new(&tmp, bytes).map_err(|e| e.to_string())?;
    let back = match std::fs::read(&tmp) {
        Ok(back) if back == bytes => Ok(()),
        Ok(_) => Err("the written file reads back different".to_string()),
        Err(e) => Err(e.to_string()),
    }
    .and_then(|()| match old {
        Some(m) => std::fs::set_permissions(&tmp, m.permissions()).map_err(|e| e.to_string()),
        None => Ok(()),
    });
    if let Err(e) = back.and_then(|()| std::fs::rename(&tmp, path).map_err(|e| e.to_string())) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Write `bytes` to a new file at `path`, flushed to disk. An earlier regular file there is
/// replaced; anything else (a symlink, a folder) is refused, so the write cannot be sent
/// elsewhere.
pub fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => std::fs::remove_file(path)?,
        Ok(_) => return Err(std::io::Error::other("not a regular file")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    let written = f.write_all(bytes).and_then(|()| f.sync_all());
    if written.is_err() {
        let _ = std::fs::remove_file(path);
    }
    written
}

/// CRC-32 (IEEE), as zip stores it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// `files` in a zip archive, stored uncompressed, dated 1980-01-01.
pub fn zip(files: &[File]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    let u16le = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let u32le = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in files {
        let (crc, len, offset) = (crc32(data), data.len() as u32, out.len() as u32);
        // Version 2.0, no flags, stored, time 0, date 1980-01-01.
        let common = |v: &mut Vec<u8>| {
            for x in [20, 0, 0, 0, 0x21] {
                u16le(v, x);
            }
            for x in [crc, len, len] {
                u32le(v, x);
            }
            u16le(v, name.len() as u16);
            u16le(v, 0);
        };
        u32le(&mut out, 0x0403_4b50);
        common(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        u32le(&mut central, 0x0201_4b50);
        u16le(&mut central, 20);
        common(&mut central);
        for x in [0, 0, 0] {
            u16le(&mut central, x);
        }
        u32le(&mut central, 0);
        u32le(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let (at, size) = (out.len() as u32, central.len() as u32);
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    for x in [0, 0, files.len() as u16, files.len() as u16] {
        u16le(&mut out, x);
    }
    u32le(&mut out, size);
    u32le(&mut out, at);
    u16le(&mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn fixture(name: &str) -> Vec<u8> {
        vxr_vox::fixtures::all().into_iter().find(|f| f.name == name).unwrap().builder.build().write()
    }

    /// Catches: a save that read back with an object moved to another shown layer (the
    /// same shown voxels), with other voxels or with another colour in a slot was written
    /// as if nothing had changed, or refused without saying what differs.
    #[test]
    fn a_save_that_reads_back_different_is_refused() {
        let bytes = fixture("22-world");
        let scene = Scene::from_vox(&bytes).unwrap();
        assert_eq!(vox_bytes(&scene).unwrap(), bytes);

        let mut doc = vxr_vox::Document::read(&bytes).unwrap();
        let mut w = crate::World::from_doc(&doc).unwrap();
        let slab = (0..w.tree.nodes.len()).find(|&i| w.tree.nodes[i].name.as_deref() == Some(b"slab")).unwrap();
        assert_eq!(w.tree.nodes[slab].layer, 0);
        w.tree.nodes[slab].layer = 1;
        w.save_into(&mut doc).unwrap();
        let relayered = doc.write();
        assert_eq!(Scene::from_vox(&relayered).unwrap().world_voxels(true), scene.world_voxels(true));
        assert_eq!(
            reads_back_as(&relayered, &scene).unwrap_err(),
            "the result reads back with a different name, hidden flag or layer on \"slab\" (shown, on layer 0)"
        );

        assert_eq!(
            reads_back_as(&fixture("12-layers"), &scene).unwrap_err(),
            "the result reads back with different shown voxels: [-4, 0, -3] in \"slab\" is empty, not colour 8"
        );

        let mut recoloured = Scene::from_vox(&bytes).unwrap();
        recoloured.palette[5][1] ^= 1;
        assert_eq!(reads_back_as(&bytes, &recoloured).unwrap_err(), "the result reads back with a different colour 5");
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    /// Read a stored zip back through its central directory.
    fn unzip(z: &[u8]) -> Vec<(String, Vec<u8>)> {
        let u16at = |i: usize| u16::from_le_bytes([z[i], z[i + 1]]) as usize;
        let u32at = |i: usize| u32::from_le_bytes(z[i..i + 4].try_into().unwrap()) as usize;
        let end = z.len() - 22;
        assert_eq!(u32at(end), 0x0605_4b50);
        let (n, mut c) = (u16at(end + 10), u32at(end + 16));
        let mut out = Vec::new();
        for _ in 0..n {
            assert_eq!(u32at(c), 0x0201_4b50);
            let (crc, len, nl, at) = (u32at(c + 16), u32at(c + 24), u16at(c + 28), u32at(c + 42));
            let name = String::from_utf8(z[c + 46..c + 46 + nl].to_vec()).unwrap();
            assert_eq!(u32at(at), 0x0403_4b50);
            let data = z[at + 30 + nl..at + 30 + nl + len].to_vec();
            assert_eq!(crc32(&data) as usize, crc);
            out.push((name, data));
            c += 46 + nl;
        }
        out
    }

    #[test]
    fn zip_round_trips() {
        let files = vec![("a.obj".to_string(), b"v 0 0 0\n".to_vec()), ("a.png".to_string(), vec![0, 1, 2, 255])];
        assert_eq!(unzip(&zip(&files)), files);
    }

    #[test]
    fn obj_files_name_each_other_and_skip_an_empty_scene() {
        let mut s = Scene::new([4, 4, 4]);
        assert!(obj_files(&s, "x").is_err());
        s.edit([([1, 1, 1], 3)]);
        let (files, quads) = obj_files(&s, "rock").unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(names, ["rock.obj", "rock.mtl", "rock.png"]);
        assert_eq!(quads, 6);
        assert!(String::from_utf8_lossy(&files[0].1).contains("mtllib rock.mtl"));
        assert!(String::from_utf8_lossy(&files[1].1).contains("map_Kd rock.png"));
        assert_eq!(&files[2].1[1..4], b"PNG");
    }

    #[test]
    fn obj_files_name_each_other_without_whitespace() {
        let mut s = Scene::new([4, 4, 4]);
        s.edit([([1, 1, 1], 3)]);
        for (stem, want) in [(" my  house\t#2 ", "my-house-2"), ("#", "vxr")] {
            let (files, _) = obj_files(&s, stem).unwrap();
            let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
            assert_eq!(names, [format!("{want}.obj"), format!("{want}.mtl"), format!("{want}.png")]);
            let line = |i: usize, key: &str| {
                String::from_utf8_lossy(&files[i].1).lines().find_map(|l| l.strip_prefix(key)).unwrap().to_string()
            };
            assert_eq!([line(0, "mtllib "), line(1, "map_Kd ")], [names[1], names[2]]);
        }
    }

    #[test]
    fn slices_png_is_one_strip_of_layers_and_skips_an_empty_scene() {
        let mut s = Scene::new([4, 3, 5]);
        assert!(slices_png(&s).is_err());
        s.edit([([1, 1, 1], 3)]);
        let (bytes, size, layers) = slices_png(&s).unwrap();
        assert_eq!((size, layers), ([12, 5], 3));
        assert_eq!(&bytes[1..4], b"PNG");
    }
}
