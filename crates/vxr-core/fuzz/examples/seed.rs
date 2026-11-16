//! Write the generated fixtures, the fuzz regressions and the prototype island (when
//! built) as seeds: `cargo run --example seed -- corpus/scene_from_vox`. With `--bricks`
//! before the folder, write the same scenes as brick scene files, for `bricks_decode`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (bricks, dir) = match args.as_slice() {
        [dir] => (false, dir.clone()),
        [flag, dir] if flag == "--bricks" => (true, dir.clone()),
        _ => {
            eprintln!("usage: seed [--bricks] <dir>");
            std::process::exit(2);
        }
    };
    std::fs::create_dir_all(&dir).unwrap();
    if bricks {
        let root = env!("CARGO_MANIFEST_DIR");
        let mut files: Vec<(String, Vec<u8>)> =
            vxr_vox::fixtures::all().into_iter().map(|f| (f.name.to_string(), f.builder.build().write())).collect();
        if let Ok(island) = std::fs::read(format!("{root}/../../../prototype/demo/scene.vox")) {
            files.push(("island".into(), island));
        }
        for (name, vox) in files {
            // A file with several objects opens as the composed world.
            if let Ok(scene) = vxr_core::Scene::from_vox(&vox) {
                std::fs::write(format!("{dir}/{name}.vxrb"), vxr_core::bricks::encode(&scene.map, &scene.palette))
                    .unwrap();
            }
        }
        return;
    }
    for f in vxr_vox::fixtures::all() {
        std::fs::write(format!("{dir}/{}.vox", f.name), f.builder.build().write()).unwrap();
    }
    let root = env!("CARGO_MANIFEST_DIR");
    for rel in ["../../vxr-vox/tests/regressions"] {
        let Ok(rd) = std::fs::read_dir(format!("{root}/{rel}")) else { continue };
        for e in rd.flatten() {
            std::fs::copy(e.path(), format!("{dir}/{}", e.file_name().to_string_lossy())).unwrap();
        }
    }
    let _ = std::fs::copy(format!("{root}/../../../prototype/demo/scene.vox"), format!("{dir}/island.vox"));
    // The third-party corpus, when `spikes/corpus/fetch-corpus` has fetched it.
    let mut n = 0;
    let mut stack = vec![std::path::PathBuf::from(format!("{root}/../../../spikes/corpus/fetched"))];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vox")) {
                n += 1;
                std::fs::copy(&p, format!("{dir}/corpus-{n}.vox")).unwrap();
            }
        }
    }
}
