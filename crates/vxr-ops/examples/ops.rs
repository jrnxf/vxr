//! Print what a model gets: `ops tools` (every tool with its JSON Schema), `ops summary
//! <scene.vox>` (the scene summary) or `ops parts <scene.vox>` (every part).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let scene = |path: &str| {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let scene = vxr_core::Scene::from_vox(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        let session = vxr_ops::Session::new(&scene, Some(&bytes));
        (scene, session)
    };
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["tools"] => println!("{}", serde_json::to_string_pretty(&vxr_ops::tools()).unwrap()),
        ["summary", path] => {
            let (s, o) = scene(path);
            println!("{}", o.summary(&s));
        }
        ["parts", path] => {
            let (mut s, mut o) = scene(path);
            println!("{}", o.call(&mut s, "find_parts", serde_json::json!({})).unwrap());
        }
        _ => eprintln!("usage: ops tools | summary <scene.vox> | parts <scene.vox>"),
    }
}
