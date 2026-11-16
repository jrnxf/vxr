//! Model exports as files: OBJ with its `.mtl` and palette texture, PNG slices, the embed
//! folder (a page with the `<voxel-scene>` tag and everything it loads), and a
//! stored (not compressed) zip to hand several files to a browser as one download. And the
//! `.vox` save: bytes checked to read back as the scene, written so a failure mid-write
//! never leaves a part-written file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Scene;
use crate::export::{TEXTURE_W, obj, slices, sprite, sprite_size};

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

/// Pixels per voxel step of the isometric sprite export: one voxel is 4 pixels wide and 3
/// tall, the smallest pixel-art cube. A larger sprite is this one scaled by whole numbers.
pub const SPRITE_SCALE: u32 = 1;

/// The isometric sprite of what the view shows (the edited model, or every shown object in
/// the world) as one PNG with a transparent background ([`sprite`]). Returns the PNG and
/// its size.
pub fn sprite_png(scene: &Scene) -> Result<(Vec<u8>, [u32; 2]), String> {
    let size = scene.bounds().size;
    // The voxels' own box is never larger than the bounds, so this refuses before any work.
    let [w, h] = sprite_size(size, SPRITE_SCALE);
    let Some(s) = (w * h <= SLICES_MAX).then(|| sprite(&scene.map, &scene.palette, SPRITE_SCALE)) else {
        return Err(format!("{}×{}×{} is too large for one image", size[0], size[1], size[2]));
    };
    let s = s.ok_or("the scene is empty")?;
    Ok((png(s.width, s.height, &s.rgba)?, [s.width, s.height]))
}

/// The embed viewer's files, which a page with a `<voxel-scene>` tag loads from one folder:
/// the loader the page includes, then the glue and the wasm it fetches
/// (`crates/vxr-embed/build` makes them).
pub const EMBED_VIEWER: [&str; 3] = ["voxel-scene.js", "vxr_embed.js", "vxr_embed_bg.wasm"];

/// The poster's size in pixels: the tag's 16:10 shape, sharp at 640 CSS pixels wide on a
/// screen with two pixels per point.
pub const EMBED_POSTER: (u32, u32) = (1280, 800);

/// The name an embed's scene file and poster share, from the scene's name: letters, digits,
/// `-` and `_` stay, every run of anything else becomes one `-`, so the name needs no
/// escaping in a URL, in HTML or in a shell.
pub fn embed_stem(name: &str) -> String {
    let name = name.strip_suffix(".vox").or_else(|| name.strip_suffix(".VOX")).unwrap_or(name);
    let parts: Vec<&str> =
        name.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() { "scene".into() } else { parts.join("-") }
}

/// Text as HTML text or a quoted attribute's value.
fn html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// What a page needs to show the scene named `name` from an embed folder beside it: the
/// loader's script and the tag, with the poster as its image so it paints before any script.
pub fn embed_snippet(name: &str) -> String {
    let stem = embed_stem(name);
    let alt = name.strip_suffix(".vox").or_else(|| name.strip_suffix(".VOX")).unwrap_or(name).trim();
    let alt = if alt.is_empty() { "A voxel scene".to_string() } else { html(alt) };
    format!(
        "<script src=\"{}\" async></script>\n<voxel-scene src=\"{stem}.vxrb\" alt=\"{alt}\">\n  <img src=\"{stem}.png\" alt=\"\">\n</voxel-scene>",
        EMBED_VIEWER[0]
    )
}

/// The embed folder of what the view shows (the edited model, or every shown object in the
/// world): `index.html` showing the tag, the scene as a brick scene file, `poster` (a PNG)
/// and the viewer's files (`viewer`, named as [`EMBED_VIEWER`]). The page and the scene are
/// first, so a reader of the list finds them there.
pub fn embed_files(scene: &Scene, name: &str, poster: Vec<u8>, viewer: Vec<File>) -> Result<Vec<File>, String> {
    if scene.map.counts[1..].iter().all(|&n| n == 0) {
        return Err("the scene is empty".into());
    }
    if let Some(missing) = EMBED_VIEWER.iter().find(|n| !viewer.iter().any(|f| f.0 == **n)) {
        return Err(format!("the viewer file {missing} is missing"));
    }
    let stem = embed_stem(name);
    let snippet = embed_snippet(name);
    let title = html(name.strip_suffix(".vox").or_else(|| name.strip_suffix(".VOX")).unwrap_or(name).trim());
    let title = if title.is_empty() { "A voxel scene".to_string() } else { title };
    let page = format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{title}</title>
    <style>
      body {{ margin: 0 auto; padding: 24px; max-width: 800px; font: 15px/1.5 system-ui, sans-serif; background: #fafaf7; color: #222; }}
      pre {{ padding: 12px; border-radius: 8px; background: #efede6; overflow-x: auto; font: 13px/1.5 ui-monospace, monospace; }}
      #file {{ padding: 12px; border-radius: 8px; background: #fde8c8; }}
      @media (prefers-color-scheme: dark) {{
        body {{ background: #161616; color: #e8e6e0; }}
        pre {{ background: #242424; }}
        #file {{ background: #4a3410; }}
      }}
    </style>
    <script src="{loader}" async></script>
  </head>
  <body>
    <h1>{title}</h1>
    <p id="file" hidden>
      Browsers do not run the viewer from a page opened as a file, so only the picture shows. Serve this
      folder over HTTP: for example run <code>python3 -m http.server</code> in it and open
      <code>http://localhost:8000</code>.
    </p>
    {tag}
    <p>To put this scene on a page, copy every file of this folder but <code>index.html</code> beside the page and paste this into it:</p>
    <pre>{shown}</pre>
    <p>The scene needs WebGPU. A browser without it shows the picture.</p>
    <script>
      if (location.protocol === 'file:') document.getElementById('file').hidden = false;
    </script>
  </body>
</html>
"#,
        loader = EMBED_VIEWER[0],
        tag = snippet.split_once('\n').map_or("", |s| s.1).replace('\n', "\n    "),
        // Line by line: `html` turns a line end, like any control character, into a space.
        shown = snippet.lines().map(html).collect::<Vec<_>>().join("\n"),
    );
    let mut files = vec![
        ("index.html".to_string(), page.into_bytes()),
        (format!("{stem}.vxrb"), crate::bricks::encode(&scene.map, &scene.palette)),
        (format!("{stem}.png"), poster),
    ];
    files.extend(viewer.into_iter().filter(|f| EMBED_VIEWER.contains(&f.0.as_str())));
    Ok(files)
}

/// Seconds a turntable's animated file takes for one full turn, whatever its count of frames.
pub const TURN_SECONDS: u16 = 4;

/// A turntable's frames as the files of its folder, and the folder's name: `<stem>-turntable`
/// holding `<stem>-01.png` and so on, the stem as [`embed_stem`] makes it, then `<stem>.png`,
/// the same frames as one animated PNG that loops ([`apng`], one turn in [`TURN_SECONDS`]).
/// The numbers start at 1 and are padded to one width (at least two digits), so the frames
/// sort by name and `ffmpeg -i <stem>-%02d.png` reads them.
pub fn turntable_files(name: &str, frames: Vec<Vec<u8>>) -> Result<(String, Vec<File>), String> {
    let stem = embed_stem(name);
    let width = frames.len().to_string().len().max(2);
    let turn = apng(&frames, TURN_SECONDS, frames.len().min(u16::MAX as usize) as u16)?;
    let mut files: Vec<File> =
        frames.into_iter().enumerate().map(|(i, png)| (format!("{stem}-{:0width$}.png", i + 1), png)).collect();
    files.push((format!("{stem}.png"), turn));
    Ok((format!("{stem}-turntable"), files))
}

/// PNG files of one size and pixel format as one animated PNG (APNG) that loops for ever,
/// each frame shown for `num / den` seconds. Nothing is compressed again: every frame's
/// `IDAT` data is carried over as it is, so the cost is a copy. The first frame is also the
/// still picture a viewer without APNG shows.
pub fn apng(frames: &[Vec<u8>], num: u16, den: u16) -> Result<Vec<u8>, String> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[&[u8]]) {
        let len: usize = data.iter().map(|d| d.len()).sum();
        out.extend((len as u32).to_be_bytes());
        let from = out.len();
        out.extend(kind);
        data.iter().for_each(|d| out.extend(*d));
        let crc = crc32(&out[from..]);
        out.extend(crc.to_be_bytes());
    }
    /// A PNG chunk: its type and its data.
    type Chunk<'a> = (&'a [u8], &'a [u8]);
    /// The chunks of a PNG file, up to `IEND`.
    fn chunks(png: &[u8]) -> Result<Vec<Chunk<'_>>, String> {
        let mut rest = png.strip_prefix(SIGNATURE).ok_or("a frame is not a PNG file")?;
        let mut out = Vec::new();
        loop {
            let cut = "a frame's PNG file is cut short";
            let (head, body) = rest.split_at_checked(8).ok_or(cut)?;
            let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
            // The data and its CRC.
            let (data, after) = body.split_at_checked(len).ok_or(cut)?;
            rest = after.get(4..).ok_or(cut)?;
            if &head[4..] == b"IEND" {
                return Ok(out);
            }
            out.push((&head[4..], data));
        }
    }
    if frames.is_empty() {
        return Err("no frames to animate".into());
    }
    let mut out = SIGNATURE.to_vec();
    let (mut header, mut seq) = (None, 0u32);
    for (i, frame) in frames.iter().enumerate() {
        let chunks = chunks(frame)?;
        let ihdr =
            chunks.iter().find(|c| c.0 == b"IHDR").filter(|c| c.1.len() == 13).ok_or("a frame has no PNG header")?.1;
        match header {
            None => {
                // Everything before the pixels (the header and, for a file not made here,
                // its palette or colour space) comes from the first frame.
                for (kind, data) in chunks.iter().take_while(|c| c.0 != b"IDAT") {
                    chunk(&mut out, kind, &[data]);
                    if *kind == b"IHDR" {
                        chunk(&mut out, b"acTL", &[&(frames.len() as u32).to_be_bytes(), &0u32.to_be_bytes()]);
                    }
                }
                header = Some(ihdr);
            }
            Some(first) if first != ihdr => return Err("the frames differ in size or pixel format".into()),
            Some(_) => {}
        }
        // Frame control: the whole picture, replaced each time (no dispose, no blend).
        let mut control = seq.to_be_bytes().to_vec();
        control.extend(&ihdr[..8]);
        control.extend([0; 8]);
        control.extend(num.to_be_bytes());
        control.extend(den.to_be_bytes());
        control.extend([0, 0]);
        chunk(&mut out, b"fcTL", &[&control]);
        seq += 1;
        let before = seq;
        for (_, data) in chunks.iter().filter(|c| c.0 == b"IDAT") {
            if i == 0 {
                chunk(&mut out, b"IDAT", &[data]);
            } else {
                chunk(&mut out, b"fdAT", &[&seq.to_be_bytes(), data]);
                seq += 1;
            }
        }
        if i == 0 && !chunks.iter().any(|c| c.0 == b"IDAT") || i > 0 && seq == before {
            return Err("a frame has no pixels".into());
        }
    }
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
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

/// CRC-32 (IEEE), as zip stores it, a byte at a time through a table: a whole library is up to
/// 100 MB.
fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    !bytes.iter().fold(!0u32, |c, &b| TABLE[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// `files` in a zip archive, stored uncompressed, dated 1980-01-01, the names in UTF-8.
pub fn zip(files: &[File]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    let u16le = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let u32le = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in files {
        let (crc, len, offset) = (crc32(data), data.len() as u32, out.len() as u32);
        // Version 2.0, names in UTF-8 (flag bit 11), stored, time 0, date 1980-01-01.
        let common = |v: &mut Vec<u8>| {
            for x in [20, 0x0800, 0, 0, 0x21] {
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
    fn turntable_frames_sort_by_name_in_frame_order() {
        for n in [1, 8, 64, 100, 1000] {
            let frames: Vec<Vec<u8>> =
                (0..n as u32).map(|i| png(1, 1, &[i as u8, (i >> 8) as u8, 0, 255]).unwrap()).collect();
            let (folder, mut files) = turntable_files("my house #2.vox", frames.clone()).unwrap();
            assert_eq!(folder, "my-house-2-turntable");
            // The animated file is last, by order and by name, and holds every frame.
            let turn = files.pop().unwrap();
            assert_eq!(turn.0, "my-house-2.png");
            assert_eq!(animation(&turn.1).0.len(), n);
            let mut sorted = files.clone();
            sorted.push(turn.clone());
            sorted.sort();
            assert_eq!(sorted.pop(), Some(turn));
            assert_eq!(sorted, files, "{n} frames");
            assert_eq!(files.iter().map(|f| f.1.clone()).collect::<Vec<_>>(), frames);
            let last = if n < 100 { format!("{n:02}") } else { n.to_string() };
            assert_eq!(files[n - 1].0, format!("my-house-2-{last}.png"));
        }
    }

    /// An animated PNG read back by the `png` crate (which checks every CRC and sequence
    /// number): each frame's pixels, its delay as a fraction, and the count of plays.
    fn animation(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<(u16, u16)>, u32) {
        let mut r = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let control = *r.info().animation_control().expect("an animation");
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let (mut frames, mut delays) = (Vec::new(), Vec::new());
        for _ in 0..control.num_frames {
            let n = r.next_frame(&mut buf).unwrap().buffer_size();
            let f = r.info().frame_control().unwrap();
            assert_eq!((f.width, f.height, f.x_offset, f.y_offset), (r.info().width, r.info().height, 0, 0));
            assert_eq!((f.dispose_op, f.blend_op), (png::DisposeOp::None, png::BlendOp::Source));
            frames.push(buf[..n].to_vec());
            delays.push((f.delay_num, f.delay_den));
        }
        (frames, delays, control.num_plays)
    }

    /// `png` with its pixel data cut into `parts` `IDAT` chunks, as other encoders write it.
    fn split_idat(png: &[u8], parts: usize) -> Vec<u8> {
        let mut out = png[..8].to_vec();
        let mut at = 8;
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let (kind, data) = (&png[at + 4..at + 8], &png[at + 8..at + 8 + len]);
            let pieces: Vec<&[u8]> =
                if kind == b"IDAT" { data.chunks(len.div_ceil(parts).max(1)).collect() } else { vec![data] };
            for piece in pieces {
                out.extend((piece.len() as u32).to_be_bytes());
                let from = out.len();
                out.extend(kind);
                out.extend(piece);
                out.extend(crc32(&out[from..]).to_be_bytes());
            }
            at += 12 + len;
        }
        out
    }

    proptest::proptest! {
        /// Every frame of the animated file is the frame that went in, in order, however an
        /// encoder cut its pixel data into chunks, and the file loops with the given delay.
        #[test]
        fn apng_holds_every_frame_in_order(
            (w, h) in (1u32..12, 1u32..12),
            seeds in proptest::collection::vec((proptest::prelude::any::<u64>(), 1usize..4), 1..7),
            (num, den) in (1u16..9, 1u16..70),
        ) {
            let pixels: Vec<Vec<u8>> = seeds
                .iter()
                .map(|&(seed, _)| {
                    (0..w * h * 4).map(|i| (seed.wrapping_mul(i as u64 * 2 + 1).wrapping_add(seed >> 7) >> 24) as u8).collect()
                })
                .collect();
            let frames: Vec<Vec<u8>> =
                pixels.iter().zip(&seeds).map(|(px, &(_, parts))| split_idat(&png(w, h, px).unwrap(), parts)).collect();
            let file = apng(&frames, num, den).unwrap();
            let (read, delays, plays) = animation(&file);
            proptest::prop_assert_eq!(&read, &pixels);
            proptest::prop_assert_eq!(delays, vec![(num, den); frames.len()]);
            proptest::prop_assert_eq!(plays, 0);
            // A viewer without APNG reads the first frame from the same file.
            let mut still = png::Decoder::new(std::io::Cursor::new(&file)).read_info().unwrap();
            let mut buf = vec![0; still.output_buffer_size().unwrap()];
            let n = still.next_frame(&mut buf).unwrap().buffer_size();
            proptest::prop_assert_eq!(&buf[..n], &pixels[0][..]);
        }
    }

    #[test]
    fn apng_refuses_frames_it_cannot_join() {
        let (a, b) = (png(2, 2, &[9; 16]).unwrap(), png(2, 1, &[9; 8]).unwrap());
        let no_pixels = [&a[..33], &a[a.len() - 12..]].concat();
        for (frames, want) in [
            (vec![], "no frames to animate"),
            (vec![a.clone(), b], "the frames differ in size or pixel format"),
            (vec![a.clone(), b"GIF89a".to_vec()], "a frame is not a PNG file"),
            (vec![a.clone(), a[..a.len() - 13].to_vec()], "a frame's PNG file is cut short"),
            (vec![a.clone(), no_pixels.clone()], "a frame has no pixels"),
            (vec![no_pixels], "a frame has no pixels"),
        ] {
            assert_eq!(apng(&frames, 1, 10).unwrap_err(), want);
        }
    }

    #[test]
    fn zip_round_trips() {
        let files = vec![("a.obj".to_string(), b"v 0 0 0\n".to_vec()), ("a.png".to_string(), vec![0, 1, 2, 255])];
        assert_eq!(unzip(&zip(&files)), files);
    }

    #[test]
    fn zip_marks_names_as_utf8() {
        let files = vec![("Château.vox".to_string(), vec![7; 3])];
        let z = zip(&files);
        assert_eq!(u16::from_le_bytes([z[6], z[7]]), 0x0800);
        assert_eq!(unzip(&z), files);
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

    #[test]
    fn sprite_png_is_the_voxels_on_a_transparent_background_and_skips_an_empty_scene() {
        let mut s = Scene::new([4, 3, 5]);
        assert_eq!(sprite_png(&s).unwrap_err(), "the scene is empty");
        s.palette[3] = [200, 100, 50, 255];
        s.edit([([1, 1, 1], 3), ([1, 2, 1], 3), ([2, 1, 1], 3)]);
        let (bytes, size) = sprite_png(&s).unwrap();
        // Two voxels along x and two up, one deep: 2 * 3 wide, 3 + 2 * 2 - 1 tall.
        assert_eq!(size, [6, 6]);
        let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes)).read_info().unwrap();
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut px).unwrap();
        assert_eq!((info.width, info.height, info.color_type), (6, 6, png::ColorType::Rgba));
        let rows: Vec<&[u8]> = px[..info.buffer_size()].chunks(24).collect();
        // The top row: the upper voxel's top face, then nothing over the lower voxel.
        assert_eq!(rows[0], [[200, 100, 50, 255]; 4].concat().iter().chain(&[0; 8]).copied().collect::<Vec<u8>>());
        assert_eq!(rows[5][..8], [0; 8]);
        assert_eq!(rows[5][16..], [160, 80, 40, 255, 160, 80, 40, 255]);
        // A scene whose sprite would pass the limit is refused before it is drawn.
        let big = Scene::new([4000, 4000, 4000]);
        assert!(sprite_png(&big).unwrap_err().contains("too large"));
    }
}
