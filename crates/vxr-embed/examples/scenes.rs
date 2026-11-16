//! Write the scenes `crates/vxr-embed/check` shows, from the generated fixtures (nothing is
//! committed as a scene file): `cargo run -p vxr-embed --example scenes -- <dir>`.
//!
//! - `world.vox`: the world fixture (several objects, one on a hidden layer).
//! - `world.vxrb`: what the embed shows of it, as a brick scene file.
//! - `cut.vxrb`: that file cut short, which the viewer must refuse.

use vxr_core::bricks;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: scenes <dir>");
    let vox = vxr_vox::fixtures::all()
        .into_iter()
        .find(|f| f.name == "22-world")
        .expect("the world fixture")
        .builder
        .build()
        .write();
    let (map, palette) = bricks::open(&vox).expect("the fixture opens");
    let file = bricks::encode(&map, &palette);
    std::fs::create_dir_all(&dir).expect("the folder");
    std::fs::write(format!("{dir}/world.vox"), &vox).expect("world.vox");
    std::fs::write(format!("{dir}/cut.vxrb"), &file[..file.len() / 2]).expect("cut.vxrb");
    std::fs::write(format!("{dir}/world.vxrb"), &file).expect("world.vxrb");
    println!("world: {} voxels, {} B as .vox, {} B as bricks", map.voxels, vox.len(), file.len());
}
