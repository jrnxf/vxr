//! Write one generated fixture as a `.vox` file, for checks outside Rust (the browser
//! check opens `22-world` through the page's file input):
//! `cargo run -p vxr-vox --example fixture -- <name> <out.vox>`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [name, out] = args.as_slice() else {
        eprintln!("usage: fixture <name> <out.vox>");
        std::process::exit(2);
    };
    let Some(f) = vxr_vox::fixtures::all().into_iter().find(|f| f.name == name) else {
        eprintln!("no fixture named {name}");
        std::process::exit(2);
    };
    std::fs::write(out, f.builder.build().write()).unwrap_or_else(|e| {
        eprintln!("{out}: {e}");
        std::process::exit(1);
    });
}
