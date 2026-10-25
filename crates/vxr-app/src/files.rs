//! Model exports as files: OBJ with its `.mtl` and palette texture, and a stored (not
//! compressed) zip to hand several files to a browser as one download.

use std::collections::BTreeMap;

use vxr_core::Scene;
use vxr_core::export::{TEXTURE_W, obj};

use crate::render::png;

/// A file to write or download: its name and bytes.
pub type File = (String, Vec<u8>);

/// The OBJ export of what the view shows (the edited model, or every shown object in the
/// world), named `<stem>.obj`, `<stem>.mtl` and `<stem>.png`, and its quad count.
pub fn obj_files(scene: &Scene, stem: &str) -> Result<(Vec<File>, usize), String> {
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
}
