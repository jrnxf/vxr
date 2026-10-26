//! `chunkdiff <before.vox> <after.vox> [--expect <key>]...`: which chunks an edit changed.
//! `chunkdiff --list <file.vox>` lists a file's chunk keys.
//!
//! Each chunk gets a key: its id plus what it belongs to (`MATL 8`, `rCAM 0`, `LAYR 1`,
//! `nTRN 5` for scene graph node ids, `XYZI #0` and `SIZE #0` by model order, `rOBJ #1` by
//! order, `RGBA`, `NOTE`, ...). Every key is reported as identical (same bytes), changed,
//! removed or added. Keys passed with `--expect` may change; any other change fails (exit
//! 1); a key ending in `*` matches every key it starts with. It also opens `after.vox` with `vxr_core::Scene::from_vox`, the editor's loader.

use std::collections::BTreeMap;

use vxr_vox::chunk::{IdDict, Ntrn};
use vxr_vox::{Chunk, RawFile, id_str};

fn keys(raw: &RawFile) -> Vec<(String, &Chunk)> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    raw.chunks
        .iter()
        .map(|c| {
            let id = id_str(&c.id);
            let by_id = || c.content.get(0..4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
            let key = match &c.id {
                b"nTRN" | b"nGRP" | b"nSHP" | b"LAYR" | b"MATL" | b"rCAM" => match by_id() {
                    Some(n) => format!("{id} {n}"),
                    None => format!("{id} ?"),
                },
                b"SIZE" | b"XYZI" | b"TDCZ" | b"rOBJ" | b"MATT" => {
                    let n = seen.entry(id.clone()).or_default();
                    *n += 1;
                    format!("{id} #{}", *n - 1)
                }
                _ => {
                    let n = seen.entry(id.clone()).or_default();
                    *n += 1;
                    if *n == 1 { id } else { format!("{id} #{}", *n - 1) }
                }
            };
            (key, c)
        })
        .collect()
}

/// A short note on what a chunk is, for the report.
fn label(c: &Chunk) -> String {
    match &c.id {
        b"nTRN" => Ntrn::parse(&c.content)
            .ok()
            .map(|n| {
                let mut s = Vec::new();
                for k in ["_name", "_hidden"] {
                    if let Some(v) = n.attrs.get_str(k) {
                        s.push(format!("{k}={v}"));
                    }
                }
                if let Some(t) = n.frames.first().and_then(|f| f.get_str("_t")) {
                    s.push(format!("_t={t}"));
                }
                if n.layer >= 0 {
                    s.push(format!("layer {}", n.layer));
                }
                s.join(" ")
            })
            .unwrap_or_default(),
        b"LAYR" | b"MATL" | b"rCAM" => IdDict::parse(&c.id, &c.content)
            .ok()
            .map(|d| {
                d.dict
                    .0
                    .iter()
                    .map(|(k, v)| format!("{}={}", String::from_utf8_lossy(k), String::from_utf8_lossy(v)))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default(),
        _ => format!("{} bytes", c.encoded_len()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files = Vec::new();
    let mut expect = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--expect" {
            expect.push(it.next().expect("--expect needs a key").clone());
        } else {
            files.push(a.clone());
        }
    }
    if files.first().map(String::as_str) == Some("--list") {
        let bytes = std::fs::read(&files[1]).expect("file");
        let raw = RawFile::parse(&bytes).expect("parses");
        for (k, c) in keys(&raw) {
            println!("{k}: {}", label(c));
        }
        return;
    }
    let [before, after] = files.as_slice() else {
        eprintln!("usage: chunkdiff <before.vox> <after.vox> [--expect <key>]...");
        std::process::exit(2);
    };
    let read = |p: &str| std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let (a_bytes, b_bytes) = (read(before), read(after));
    let a = RawFile::parse(&a_bytes).expect("before parses");
    let b = RawFile::parse(&b_bytes).expect("after parses");
    let (ka, kb) = (keys(&a), keys(&b));
    let ma: BTreeMap<&str, &Chunk> = ka.iter().map(|(k, c)| (k.as_str(), *c)).collect();
    let mb: BTreeMap<&str, &Chunk> = kb.iter().map(|(k, c)| (k.as_str(), *c)).collect();
    let (mut same, mut bad, mut ok) = (0, Vec::new(), Vec::new());
    let matches = |e: &str, key: &str| match e.strip_suffix('*') {
        Some(prefix) => key.starts_with(prefix),
        None => e == key,
    };
    let mut note = |key: &str, what: String| {
        if expect.iter().any(|e| matches(e, key)) { ok.push(what) } else { bad.push(what) }
    };
    for (k, c) in &ka {
        match mb.get(k.as_str()) {
            Some(d) if *d == *c => same += 1,
            Some(d) => note(k, format!("changed {k}: {} -> {}", label(c), label(d))),
            None => note(k, format!("removed {k}: {}", label(c))),
        }
    }
    for (k, c) in &kb {
        if !ma.contains_key(k.as_str()) {
            note(k, format!("added {k}: {}", label(c)));
        }
    }
    // Do the identical chunks keep their order?
    let order = |ks: &[(String, &Chunk)], other: &BTreeMap<&str, &Chunk>| -> Vec<String> {
        ks.iter().filter(|(k, c)| other.get(k.as_str()) == Some(c)).map(|(k, _)| k.clone()).collect()
    };
    let in_order = order(&ka, &mb) == order(&kb, &ma);
    let header = a.version == b.version && a.main_content == b.main_content && a.trailing == b.trailing;
    println!("{before} -> {after}: {} -> {} chunks, {} -> {} bytes", ka.len(), kb.len(), a_bytes.len(), b_bytes.len());
    println!("identical: {same} chunks{}", if in_order { ", in the same order" } else { ", ORDER CHANGED" });
    for s in &ok {
        println!("expected   {s}");
    }
    for s in &bad {
        println!("UNEXPECTED {s}");
    }
    for e in expect.iter().filter(|e| !e.ends_with('*') && ma.get(e.as_str()) == mb.get(e.as_str())) {
        println!("note: expected {e} to change; it did not");
    }
    match vxr_core::Scene::from_vox(&b_bytes) {
        Ok(s) => println!(
            "opens in vxr-core: {} voxels in all, {} objects",
            s.world_voxels(false).len(),
            s.world().map_or(1, |w| w.objects().len())
        ),
        Err(e) => {
            println!("FAIL does not open: {e}");
            std::process::exit(1);
        }
    }
    let pass = bad.is_empty() && in_order && header;
    println!(
        "{}: {} untouched chunks byte-identical, {} expected changes, {} unexpected",
        if pass { "PASS" } else { "FAIL" },
        same,
        ok.len(),
        bad.len()
    );
    std::process::exit(if pass { 0 } else { 1 });
}
