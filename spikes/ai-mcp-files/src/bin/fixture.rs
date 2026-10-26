//! `fixture <out.vox>`: the file the edit run starts from. It is `vxr_vox::fixtures`'
//! `22-world` (named objects, a nested group, three linked copies of one tree model, three
//! layers, hidden objects) with what MagicaVoxel writes next to it: a MATL chunk for every
//! slot (the slab metal, the gem glass, orange emissive), render settings (rOBJ), three
//! saved cameras (rCAM) and palette row names (NOTE).

use vxr_vox::Dict;
use vxr_vox::fixtures::{self, GREY, MAGENTA, ORANGE};

fn main() {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: fixture <out.vox>");
        std::process::exit(2);
    };
    let mut b = fixtures::all().into_iter().find(|f| f.name == "22-world").expect("22-world fixture").builder;
    for i in 1..=255i32 {
        let d = match i as u8 {
            GREY => {
                Dict::default().with("_type", "_metal").with("_weight", "1").with("_rough", "0.2").with("_metal", "0.8")
            }
            MAGENTA => Dict::default()
                .with("_type", "_glass")
                .with("_weight", "1")
                .with("_rough", "0.1")
                .with("_ior", "0.5")
                .with("_ri", "1.5")
                .with("_trans", "0.6"),
            ORANGE => {
                Dict::default().with("_type", "_emit").with("_weight", "1").with("_emit", "0.4").with("_flux", "2")
            }
            _ => Dict::default().with("_type", "_diffuse").with("_weight", "1").with("_rough", "0.1"),
        };
        b.materials.push((i, d));
    }
    b.render_objects = vec![
        Dict::default().with("_type", "_setting").with("_ground", "1").with("_grid", "0"),
        Dict::default().with("_type", "_bloom").with("_mix", "0.5").with("_scale", "0").with("_aspect", "0"),
        Dict::default().with("_type", "_sun").with("_intensity", "0.7").with("_angle", "50 50").with("_area", "0.07"),
    ];
    let cam = |mode: &str, focus: &str, angle: &str, radius: &str| {
        Dict::default()
            .with("_mode", mode)
            .with("_focus", focus)
            .with("_angle", angle)
            .with("_radius", radius)
            .with("_frustum", "0.4")
            .with("_fov", "45")
    };
    b.cameras = vec![
        (0, cam("pers", "0 0 3", "30 45 0", "40")),
        (1, cam("orth", "0 0 0", "90 0 0", "30")),
        (2, cam("pers", "-3 3 1", "15 200 0", "12")),
    ];
    b.note = Some((0..32).map(|i| format!("row {i}").into_bytes()).collect());
    let bytes = b.build().write();
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("{out}: {e}"));
    println!("wrote {out} ({} bytes)", bytes.len());
}
