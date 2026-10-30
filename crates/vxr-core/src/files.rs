//! Model exports as files: OBJ with its `.mtl` and palette texture, PNG slices, and a
//! stored (not compressed) zip to hand several files to a browser as one download. And the
//! `.vox` save: bytes checked to read back as the scene, written so a failure mid-write
//! never leaves a part-written file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

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
/// shown ones, and for an opened file the names of its objects and groups and its layers.
/// A save that would lose any of them is an error, not a file.
pub fn vox_bytes(scene: &Scene) -> Result<Vec<u8>, String> {
    let bytes = scene.to_vox()?;
    let again = Scene::from_vox(&bytes).map_err(|e| format!("the result does not read back: {e}"))?;
    if again.world_voxels(false) != scene.world_voxels(false) || again.world_voxels(true) != scene.world_voxels(true) {
        return Err("the result reads back with different voxels".into());
    }
    if let Some(w) = scene.world()
        && again.world().map(outline) != Some(outline(w))
    {
        return Err("the result reads back with different names or layers".into());
    }
    Ok(bytes)
}

/// What a save must keep of a world besides its voxels: the names in tree order and the
/// layers. Not the tree's shape: a model over 256 on an axis is saved as a group of blocks
/// under the object's name.
fn outline(w: &crate::World) -> (Vec<Vec<u8>>, Vec<vxr_vox::LayerInfo>) {
    let names = w.tree.order().into_iter().filter_map(|i| w.tree.nodes[i].name.clone()).collect();
    (names, w.layers())
}

/// Replace `path` with `bytes` so that it holds either its old bytes or all of the new
/// ones, whatever happens mid-write: they go to `<path>.tmp`, are flushed and read back,
/// and only then is that file renamed over `path`, keeping its permissions. A symbolic
/// link at `path` is replaced, not followed.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    write_new(&tmp, bytes).map_err(|e| e.to_string())?;
    let back = match std::fs::read(&tmp) {
        Ok(back) if back == bytes => Ok(()),
        Ok(_) => Err("the written file reads back different".to_string()),
        Err(e) => Err(e.to_string()),
    }
    .and_then(|()| match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => std::fs::set_permissions(&tmp, m.permissions()).map_err(|e| e.to_string()),
        _ => Ok(()),
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
