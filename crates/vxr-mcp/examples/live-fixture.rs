//! `live-fixture <out.vox>`: write the file the live check starts from (see `vxr_mcp::fixture`).

fn main() {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: live-fixture <out.vox>");
        std::process::exit(2);
    };
    let bytes = vxr_mcp::fixture::world_with_extras();
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("{out}: {e}"));
    println!("wrote {out} ({} bytes)", bytes.len());
}
