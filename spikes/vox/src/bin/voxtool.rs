//! Spike CLI. Run `voxtool help` for the commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use vox::chunk::Typed;
use vox::scene::{Rule, Scene};
use vox::{Document, RawFile, id_str};

const HELP: &str = "voxtool <command> ...
  roundtrip <file|dir>...     unedited re-save of every .vox file: byte-identical or not
  chunks <file>               list chunks
  dump <file>                 canonical scene dump (same format as the ogt_vox harness)
  edit-check <out-dir> <file|dir>...
                              set one voxel, recolour one slot, move one node; check only
                              those chunks changed; write the edited files to out-dir
  rules <file|dir>...         files where the two transform composition rules disagree
  compare <ours.vox> <theirs.vox>   (or two folders: matching file names)
                              semantic compare (voxels, palette, materials, layers, world
                              transforms), for a fixture against MagicaVoxel's re-save;
                              exits non-zero if any of those differ
  fixtures <out-dir>          write the MagicaVoxel check fixtures";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("roundtrip") => roundtrip(&args[1..]),
        Some("chunks") => chunks(&args[1..]),
        Some("dump") => dump(&args[1..]),
        Some("edit-check") => edit_check(&args[1..]),
        Some("rules") => rules(&args[1..]),
        Some("compare") => compare(&args[1..]),
        Some("fixtures") => vox_fixtures(&args[1..]),
        _ => {
            println!("{HELP}");
            Ok(true)
        }
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type R = Result<bool, Box<dyn std::error::Error>>;

fn collect(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        if p.is_dir() {
            let mut es: Vec<_> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect();
            es.sort();
            for e in es {
                walk(&e, out);
            }
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vox")) {
            out.push(p.to_path_buf());
        }
    }
    for a in args {
        walk(Path::new(a), &mut out);
    }
    out
}

/// Group label for a corpus path: the folder under `fetched/`, else the parent folder.
fn source_of(p: &Path) -> String {
    let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if let Some(i) = parts.iter().position(|c| c == "fetched")
        && let Some(s) = parts.get(i + 1)
    {
        return s.clone();
    }
    p.parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

#[derive(Default)]
struct Tally {
    files: usize,
    identical: usize,
    rejected: usize,
}

fn roundtrip(args: &[String]) -> R {
    let files = collect(args);
    let mut by_source: BTreeMap<String, Tally> = BTreeMap::new();
    let mut by_version: BTreeMap<String, Tally> = BTreeMap::new();
    let mut chunk_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut rejects = Vec::new();
    let mut warn_files = 0;
    let mut warn_lines = Vec::new();
    let mut bytes_total = 0usize;
    let start = std::time::Instant::now();
    for f in &files {
        let bytes = std::fs::read(f)?;
        bytes_total += bytes.len();
        let src = source_of(f);
        let ver = if bytes.len() >= 8 && &bytes[..4] == b"VOX " {
            i32::from_le_bytes(bytes[4..8].try_into().unwrap()).to_string()
        } else {
            "not .vox".into()
        };
        let ts = by_source.entry(src).or_default();
        let tv = by_version.entry(ver).or_default();
        ts.files += 1;
        tv.files += 1;
        let doc = match Document::read(&bytes) {
            Ok(d) => d,
            Err(e) => {
                ts.rejected += 1;
                tv.rejected += 1;
                rejects.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        let out = doc.write();
        // Typed layer check: every chunk parses and re-encodes to the same bytes.
        let mut typed_ok = true;
        for c in &doc.raw.chunks {
            let t = Typed::parse(c)?;
            let e = chunk_counts.entry(id_str(&c.id)).or_default();
            e.0 += 1;
            if t.encode() != c.content {
                typed_ok = false;
                failures.push(format!("{}: {} re-encodes differently", f.display(), id_str(&c.id)));
            }
            if t.extra_len() > 0 {
                e.1 += 1;
            }
        }
        let scene = doc.scene()?;
        let (_, w) = scene.instances(Rule::Compose);
        if !scene.warnings.is_empty() || !w.is_empty() {
            warn_files += 1;
            let mut all: Vec<String> = scene.warnings.iter().chain(&w).cloned().collect();
            all.dedup();
            let n = all.len();
            all.truncate(3);
            warn_lines.push(format!(
                "{}: {}{}",
                f.display(),
                all.join("; "),
                if n > 3 { format!(" (+{} more)", n - 3) } else { String::new() }
            ));
        }
        if out == bytes && typed_ok {
            ts.identical += 1;
            tv.identical += 1;
        } else if out != bytes {
            let at = out.iter().zip(&bytes).position(|(a, b)| a != b).unwrap_or(out.len().min(bytes.len()));
            failures.push(format!("{}: differs at byte {at} (len {} vs {})", f.display(), out.len(), bytes.len()));
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!("files: {}  ({:.1} MB in {:.2} s)", files.len(), bytes_total as f64 / 1e6, secs);
    println!("\nby source              files  identical  rejected");
    for (k, t) in &by_source {
        println!("  {k:<20} {:>5}  {:>9}  {:>8}", t.files, t.identical, t.rejected);
    }
    println!("\nby version             files  identical  rejected");
    for (k, t) in &by_version {
        println!("  {k:<20} {:>5}  {:>9}  {:>8}", t.files, t.identical, t.rejected);
    }
    println!("\nchunks seen (count, with bytes past the known fields):");
    let line: Vec<String> = chunk_counts
        .iter()
        .map(|(k, (n, x))| if *x > 0 { format!("{k} {n} ({x})") } else { format!("{k} {n}") })
        .collect();
    println!("  {}", line.join(", "));
    println!("\nfiles with reader warnings: {warn_files}");
    for w in &warn_lines {
        println!("  {w}");
    }
    println!("\nrejected ({}):", rejects.len());
    for r in &rejects {
        println!("  {r}");
    }
    println!("\nround-trip failures ({}):", failures.len());
    for f in &failures {
        println!("  {f}");
    }
    Ok(failures.is_empty())
}

fn chunks(args: &[String]) -> R {
    let raw = RawFile::parse(&std::fs::read(&args[0])?)?;
    println!("version {}  chunks {}  trailing {}", raw.version, raw.chunks.len(), raw.trailing.len());
    for (i, c) in raw.chunks.iter().enumerate() {
        let desc = match Typed::parse(c)? {
            Typed::Size(s) => format!("{} {} {}", s.x, s.y, s.z),
            Typed::Xyzi(x) => format!("{} voxels", x.voxels.len()),
            Typed::Ntrn(n) => format!(
                "id {} child {} layer {} attrs {:?} frames {:?}",
                n.id,
                n.child,
                n.layer,
                lossy(&n.attrs),
                n.frames.iter().map(lossy).collect::<Vec<_>>()
            ),
            Typed::Ngrp(g) => format!("id {} children {:?}", g.id, g.children),
            Typed::Nshp(s) => format!("id {} models {:?}", s.id, s.models.iter().map(|m| m.0).collect::<Vec<_>>()),
            Typed::Layr(l) => format!("id {} {:?}", l.id, lossy(&l.attrs)),
            Typed::Matl(m) => format!("id {} {:?}", m.id, lossy(&m.dict)),
            Typed::Rcam(m) => format!("id {} {:?}", m.id, lossy(&m.dict)),
            Typed::Robj(o) | Typed::Meta(o) => format!("{:?}", lossy(&o.dict)),
            Typed::Note(n) => format!("{} names", n.names.len()),
            _ => format!("{} bytes", c.content.len()),
        };
        println!("{i:>4} {} {desc}", id_str(&c.id));
    }
    Ok(true)
}

fn lossy(d: &vox::Dict) -> Vec<(String, String)> {
    d.0.iter()
        .map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned()))
        .collect()
}

/// FNV-1a over a model's effective voxels, as (x, y, z, r, g, b, a) in x-major order.
fn model_hash(scene: &Scene, m: &vox::scene::Model) -> (usize, u64) {
    let mut h: u64 = 0xcbf29ce484222325;
    let vs = m.effective_voxels();
    for v in &vs {
        let c = scene.color(v[3]);
        for b in [v[0], v[1], v[2], c[0], c[1], c[2], c[3]] {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    (vs.len(), h)
}

pub fn dump_scene(scene: &Scene) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "version {}", scene.version).unwrap();
    writeln!(s, "models {}", scene.models.len()).unwrap();
    for (i, m) in scene.models.iter().enumerate() {
        let (n, h) = model_hash(scene, m);
        writeln!(s, "model {i} size {} {} {} voxels {n} hash {h:016x}", m.size[0], m.size[1], m.size[2]).unwrap();
    }
    let (inst, _) = scene.instances(Rule::Compose);
    writeln!(s, "instances {}", inst.len()).unwrap();
    let no_layers = scene.layers.is_empty();
    for i in &inst {
        let r = i.world.r;
        let layer = if no_layers { 0 } else { i.layer };
        writeln!(
            s,
            "instance model {} r {} {} {} {} {} {} {} {} {} t {} {} {} layer {} hidden {}",
            i.model,
            r[0][0],
            r[0][1],
            r[0][2],
            r[1][0],
            r[1][1],
            r[1][2],
            r[2][0],
            r[2][1],
            r[2][2],
            i.world.t[0],
            i.world.t[1],
            i.world.t[2],
            layer,
            i.hidden as u8
        )
        .unwrap();
    }
    s
}

fn dump(args: &[String]) -> R {
    let doc = Document::read(&std::fs::read(&args[0])?)?;
    print!("{}", dump_scene(&doc.scene()?));
    Ok(true)
}

/// The three scripted edits. Returns the edited document and the chunk indices each
/// edit says it touched (an index can shift if RGBA had to be inserted).
fn apply_edits(doc: &Document) -> Result<(Document, Vec<String>), vox::Error> {
    let mut d = doc.clone();
    let scene = d.scene()?;
    let mut done = Vec::new();
    if let Some((mi, m)) = scene.models.iter().enumerate().find(|(_, m)| m.size.iter().all(|&s| s > 0)) {
        // Flip the first voxel's colour, or add one at the origin.
        let (p, c) = match m.voxels.first() {
            Some(v) => ([v[0], v[1], v[2]], if v[3] == 7 { 8 } else { 7 }),
            None => ([0, 0, 0], 7),
        };
        if (0..3).all(|k| (p[k] as i32) < m.size[k]) {
            d.set_voxel(mi, p, c)?;
            done.push(format!("voxel edit model {mi}"));
        }
    }
    d.set_palette(1, [0x12, 0x34, 0x56, 0xff])?;
    done.push("palette recolour".into());
    // Move the transform node of the first shape.
    let (inst, _) = scene.instances(Rule::Compose);
    if let Some(id) = inst.iter().find_map(|i| i.transform_node) {
        let n = match scene.nodes.get(&id) {
            Some(vox::scene::Node::Transform(n)) => n.clone(),
            _ => unreachable!(),
        };
        let t = Scene::base_transform(&n, &mut vec![]).t;
        d.set_translation(id, [t[0] + 3, t[1] - 2, t[2] + 1])?;
        done.push(format!("node move {id}"));
    }
    Ok((d, done))
}

fn edit_check(args: &[String]) -> R {
    let out_dir = PathBuf::from(&args[0]);
    std::fs::create_dir_all(&out_dir)?;
    let files = collect(&args[1..]);
    let (mut ok, mut bad, mut skipped) = (0, 0, 0);
    let mut problems = Vec::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let bytes = std::fs::read(f)?;
        let Ok(doc) = Document::read(&bytes) else {
            skipped += 1;
            continue;
        };
        let (ed, done) = apply_edits(&doc)?;
        let before = doc.scene()?;
        let after = ed.scene()?;
        // Which chunks changed? Align by index, allowing one inserted RGBA.
        let a = &doc.raw.chunks;
        let b = &ed.raw.chunks;
        let inserted = b.len() == a.len() + 1;
        let mut changed = Vec::new();
        let mut j = 0;
        for (i, c) in b.iter().enumerate() {
            if inserted && j < a.len() && c.is(vox::chunk::RGBA) && !a[j].is(vox::chunk::RGBA) {
                changed.push(format!("+{}@{i}", id_str(&c.id)));
                continue;
            }
            if a.get(j) != Some(c) {
                changed.push(id_str(&c.id));
            }
            j += 1;
        }
        let expected: usize = done.len();
        for d in &done {
            *kinds.entry(d.split(' ').take(2).collect::<Vec<_>>().join(" ")).or_insert(0usize) += 1;
        }
        if inserted {
            *kinds.entry("RGBA inserted (file had none)".into()).or_insert(0) += 1;
        }
        // Semantic check: everything except the edits is unchanged.
        let mut issues = Vec::new();
        if changed.len() != expected {
            issues.push(format!("expected {expected} changed chunks, got {changed:?}"));
        }
        if before.models.len() != after.models.len() || before.nodes.len() != after.nodes.len() {
            issues.push("model or node count changed".into());
        }
        if before.materials != after.materials || before.layers != after.layers || before.cameras != after.cameras {
            issues.push("materials, layers or cameras changed".into());
        }
        let vdiff: usize = before
            .models
            .iter()
            .zip(&after.models)
            .map(|(x, y)| {
                let (a, b) = (x.effective_voxels(), y.effective_voxels());
                let sa: std::collections::BTreeSet<_> = a.into_iter().collect();
                let sb: std::collections::BTreeSet<_> = b.into_iter().collect();
                sa.symmetric_difference(&sb).count()
            })
            .sum();
        // Recolouring an existing voxel shows up as one removed plus one added entry.
        if vdiff > 2 {
            issues.push(format!("{vdiff} voxel entries changed"));
        }
        let moved: Option<i32> = done.iter().find_map(|d| d.strip_prefix("node move ")).and_then(|x| x.parse().ok());
        let (ib, _) = before.instances(Rule::Compose);
        let (ia, _) = after.instances(Rule::Compose);
        if ib.len() != ia.len() {
            issues.push("instance count changed".into());
        }
        for (x, y) in ib.iter().zip(&ia) {
            if x != y && (x.transform_node != moved || x.world.r != y.world.r || x.model != y.model) {
                issues.push(format!("instance of node {:?} changed but only node {moved:?} moved", x.transform_node));
            }
        }
        let nb = after.palette.iter().zip(&before.palette).filter(|(x, y)| x != y).count();
        if nb > 1 {
            issues.push(format!("{nb} palette entries changed"));
        }
        // Re-read the written file and re-save: stable.
        let written = ed.write();
        let again = Document::read(&written)?;
        if again.write() != written {
            issues.push("edited file does not re-save byte-identical".into());
        }
        if issues.is_empty() {
            ok += 1;
        } else {
            bad += 1;
            problems.push(format!("{}: {}", f.display(), issues.join("; ")));
        }
        let name = format!("{}__{}", source_of(f), f.file_name().unwrap().to_string_lossy());
        std::fs::write(out_dir.join(name), written)?;
    }
    println!("edited {} files: {ok} only touched the expected chunks, {bad} did not, {skipped} not readable", ok + bad);
    for (k, n) in &kinds {
        println!("  {k}: {n}");
    }
    for p in &problems {
        println!("  {p}");
    }
    Ok(bad == 0)
}

fn rules(args: &[String]) -> R {
    let files = collect(args);
    let (mut graph, mut differ) = (0, 0);
    for f in &files {
        let Ok(doc) = Document::read(&std::fs::read(f)?) else { continue };
        let s = doc.scene()?;
        if s.nodes.is_empty() {
            continue;
        }
        graph += 1;
        let (a, _) = s.instances(Rule::Compose);
        let (b, _) = s.instances(Rule::DotVoxExample);
        let n = a.iter().zip(&b).filter(|(x, y)| x.world != y.world).count();
        if n > 0 {
            differ += 1;
            // Heuristic evidence: hand-built scenes rarely have objects inside each other,
            // so the wrong rule tends to produce more cells claimed by two instances.
            let overlaps = |rule: Rule| {
                let (inst, _) = s.instances(rule);
                let mut seen = std::collections::HashMap::new();
                let mut both = 0usize;
                for (k, i) in inst.iter().enumerate() {
                    let m = &s.models[i.model];
                    for v in m.effective_voxels() {
                        let p = Scene::world_cell(&i.world, m.size, [v[0] as i32, v[1] as i32, v[2] as i32]);
                        if let Some(prev) = seen.insert(p, k)
                            && prev != k
                        {
                            both += 1;
                        }
                    }
                }
                (both, seen.len())
            };
            let (oa, ca) = overlaps(Rule::Compose);
            let (ob, cb) = overlaps(Rule::DotVoxExample);
            println!(
                "  {}: {n} of {} instances placed differently; cells shared by two instances: compose {oa} of {ca}, dot_vox rule {ob} of {cb}",
                f.display(),
                a.len()
            );
        }
    }
    println!("files with a scene graph: {graph}; the two rules disagree on {differ}");
    Ok(true)
}

fn compare(args: &[String]) -> R {
    let (pa, pb) = (Path::new(&args[0]), Path::new(&args[1]));
    if !(pa.is_dir() && pb.is_dir()) {
        return compare_files(pa, pb);
    }
    // Directory form: compare every .vox in the first folder with the same name in the second.
    let (mut pass, mut fail, mut missing) = (0, 0, Vec::new());
    for f in collect(&args[..1]) {
        let other = pb.join(f.file_name().unwrap());
        if !other.exists() {
            missing.push(f.file_name().unwrap().to_string_lossy().into_owned());
            continue;
        }
        println!("== {}", f.file_name().unwrap().to_string_lossy());
        if compare_files(&f, &other)? { pass += 1 } else { fail += 1 }
    }
    println!(
        "\n{pass} match, {fail} differ, {} not re-saved yet{}",
        missing.len(),
        if missing.is_empty() { String::new() } else { format!(": {}", missing.join(", ")) }
    );
    Ok(fail == 0)
}

fn compare_files(pa: &Path, pb: &Path) -> R {
    let a = Document::read(&std::fs::read(pa)?)?.scene()?;
    let b = Document::read(&std::fs::read(pb)?)?.scene()?;
    let mut all = true;
    let mut report = |name: &str, ok: bool, detail: String| {
        all &= ok;
        println!(
            "{:<16} {}{}",
            name,
            if ok { "match" } else { "DIFFERENT" },
            if detail.is_empty() { String::new() } else { format!("  {detail}") }
        );
    };
    // Voxels: world cell -> resolved colour, so renumbered models or palette slots
    // that keep the same colour still match.
    let wa: BTreeMap<_, _> = a.world_voxels(Rule::Compose).into_iter().map(|(p, c)| (p, a.color(c))).collect();
    let wb: BTreeMap<_, _> = b.world_voxels(Rule::Compose).into_iter().map(|(p, c)| (p, b.color(c))).collect();
    let only_a = wa.keys().filter(|k| !wb.contains_key(*k)).count();
    let only_b = wb.keys().filter(|k| !wa.contains_key(*k)).count();
    let recol = wa.iter().filter(|(k, v)| wb.get(*k).is_some_and(|w| w != *v)).count();
    report(
        "world voxels",
        only_a == 0 && only_b == 0 && recol == 0,
        format!(
            "{} vs {}; only in first {only_a}, only in second {only_b}, colour differs {recol}",
            wa.len(),
            wb.len()
        ),
    );
    if only_a + only_b > 0 {
        // A pure shift means a pivot or translation convention differs.
        if let (Some(pa), Some(pb)) = (wa.keys().next(), wb.keys().next()) {
            println!("                 first cell {pa:?} vs {pb:?}");
        }
    }
    // Palette by index. If only the order differs (another writer applied IMAP), say so.
    let pd: Vec<usize> = (1..=255u8).filter(|&i| a.color(i) != b.color(i)).map(|i| i as usize).collect();
    let mut pdetail = String::new();
    if !pd.is_empty() {
        let mut sa: Vec<_> = (1..=255u8).map(|i| a.color(i)).collect();
        let mut sb: Vec<_> = (1..=255u8).map(|i| b.color(i)).collect();
        sa.sort();
        sb.sort();
        pdetail = format!("{} indices differ (first {:?})", pd.len(), &pd[..pd.len().min(8)]);
        if sa == sb {
            pdetail += "; same colours in another order";
        }
    }
    report("palette", pd.is_empty(), pdetail);
    // Materials, normalised: a missing _type is _diffuse, numbers compare with a small
    // tolerance, and a key we wrote may be dropped only if it holds MagicaVoxel's default.
    let ma: BTreeMap<i32, &vox::Dict> = a.materials.iter().map(|m| (m.id, &m.dict)).collect();
    let mb: BTreeMap<i32, &vox::Dict> = b.materials.iter().map(|m| (m.id, &m.dict)).collect();
    let empty = vox::Dict::default();
    let mut mdiff = Vec::new();
    let mut mextra = 0;
    for id in ma.keys().chain(mb.keys()).copied().collect::<std::collections::BTreeSet<_>>() {
        let (x, y) = (*ma.get(&id).unwrap_or(&&empty), *mb.get(&id).unwrap_or(&&empty));
        let ty = |d: &vox::Dict| d.get_str("_type").unwrap_or("_diffuse").to_string();
        if ty(x) != ty(y) {
            mdiff.push(format!("id {id}: type {} vs {}", ty(x), ty(y)));
            continue;
        }
        for (k, v) in &x.0 {
            let k = String::from_utf8_lossy(k).into_owned();
            if k == "_type" {
                continue;
            }
            let v = String::from_utf8_lossy(v).into_owned();
            match y.get_str(&k) {
                Some(w) if same_value(&v, w) => {}
                Some(w) => mdiff.push(format!("id {id}: {k} {v} vs {w}")),
                None if is_mv_default(&k, &v) => {}
                None => mdiff.push(format!("id {id}: {k} {v} dropped")),
            }
        }
        mextra += y.0.iter().filter(|(k, _)| !x.0.iter().any(|(j, _)| j == k)).count();
    }
    report(
        "materials",
        mdiff.is_empty(),
        format!(
            "{} vs {} MATL; {} differences{}; {mextra} keys only in the second",
            ma.len(),
            mb.len(),
            mdiff.len(),
            if mdiff.is_empty() {
                String::new()
            } else {
                format!(" (first: {})", mdiff[..mdiff.len().min(3)].join(", "))
            }
        ),
    );
    // Layers: names and hidden flags must match; colours must match where both set one.
    let mut ldiff = Vec::new();
    for l in &a.layers {
        let Some(m) = b.layers.iter().find(|m| m.id == l.id) else {
            ldiff.push(format!("layer {} missing", l.id));
            continue;
        };
        let name = |d: &vox::Dict| d.get("_name").unwrap_or(b"").to_vec();
        if name(&l.attrs) != name(&m.attrs) {
            ldiff.push(format!("layer {} name", l.id));
        }
        if l.attrs.is_true("_hidden") != m.attrs.is_true("_hidden") {
            ldiff.push(format!("layer {} hidden", l.id));
        }
        if let (Some(c), Some(d)) = (l.attrs.get("_color"), m.attrs.get("_color"))
            && c != d
        {
            ldiff.push(format!("layer {} colour", l.id));
        }
    }
    report(
        "layers",
        ldiff.is_empty(),
        format!(
            "{} vs {} layers{}",
            a.layers.len(),
            b.layers.len(),
            if ldiff.is_empty() { String::new() } else { format!("; {}", ldiff[..ldiff.len().min(4)].join(", ")) }
        ),
    );
    let key = |s: &Scene| {
        let (inst, _) = s.instances(Rule::Compose);
        let mut v: Vec<_> = inst
            .iter()
            .map(|i| {
                (model_hash(s, &s.models[i.model]).1, i.world, if s.layers.is_empty() { 0 } else { i.layer }, i.hidden)
            })
            .collect();
        v.sort();
        v
    };
    let (ia, ib) = (key(&a), key(&b));
    let same = ia.iter().filter(|x| ib.contains(x)).count();
    report(
        "world transforms",
        ia == ib,
        format!(
            "{} vs {} instances, {same} identical (model content, matrix, translation, layer, hidden)",
            ia.len(),
            ib.len()
        ),
    );
    let ca: Vec<_> = a.cameras.iter().map(|c| (c.id, sorted(&c.dict))).collect();
    let cb: Vec<_> = b.cameras.iter().map(|c| (c.id, sorted(&c.dict))).collect();
    println!(
        "{:<16} {} ({} vs {}; recorded, not part of the pass line)",
        "cameras",
        if ca == cb { "match" } else { "different" },
        ca.len(),
        cb.len()
    );
    Ok(all)
}

fn same_value(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (x, y): (Vec<f64>, Vec<f64>) = (
        a.split_ascii_whitespace().filter_map(|t| t.parse().ok()).collect(),
        b.split_ascii_whitespace().filter_map(|t| t.parse().ok()).collect(),
    );
    !x.is_empty() && x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| (p - q).abs() < 1e-3)
}

/// Material values MagicaVoxel 0.99.7 leaves out or treats as the default.
fn is_mv_default(k: &str, v: &str) -> bool {
    let d = match k {
        "_weight" => "1",
        "_rough" => "0.1",
        "_ior" => "0.3",
        "_ri" => "1.3",
        "_d" => "0.05",
        _ => return false,
    };
    same_value(v, d)
}

fn sorted(d: &vox::Dict) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut v = d.0.clone();
    v.sort();
    v
}

fn vox_fixtures(args: &[String]) -> R {
    let dir = PathBuf::from(&args[0]);
    std::fs::create_dir_all(&dir)?;
    let mut md = String::from(
        "# What each fixture should look like in MagicaVoxel\n\nGenerated by `voxtool fixtures fixtures`. Positions are MagicaVoxel world coordinates, Z up.\n\n| File | What MagicaVoxel should show |\n|---|---|\n",
    );
    for f in vox::fixtures::all() {
        let bytes = f.builder.build().write();
        // Every fixture must survive our own round trip first.
        let doc = Document::read(&bytes)?;
        assert_eq!(doc.write(), bytes, "{} round trip", f.name);
        std::fs::write(dir.join(format!("{}.vox", f.name)), &bytes)?;
        md += &format!("| `{}.vox` | {} |\n", f.name, f.expect);
        println!("{:<28} {:>9} bytes", f.name, bytes.len());
    }
    std::fs::write(dir.join("EXPECTED.md"), md)?;
    Ok(true)
}
