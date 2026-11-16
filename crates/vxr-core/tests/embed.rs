//! The embed folder: the page, the scene file and the names that tie them together.

use std::collections::BTreeMap;

use vxr_core::files::{EMBED_VIEWER, File, embed_files, embed_snippet, embed_stem};
use vxr_core::{BrickMap, Scene, bricks};

fn viewer() -> Vec<File> {
    EMBED_VIEWER.iter().map(|n| (n.to_string(), n.as_bytes().to_vec())).collect()
}

/// Every voxel with its colour, which is what a viewer draws.
fn drawn(map: &BrickMap, palette: &[[u8; 4]; 256]) -> BTreeMap<[i32; 3], [u8; 4]> {
    let mut out = BTreeMap::new();
    map.for_each(|p, c| {
        out.insert(p, palette[c as usize]);
    });
    out
}

/// The values of every `src="..."` in `html`.
fn srcs(html: &str) -> Vec<&str> {
    html.split("src=\"").skip(1).map(|rest| rest.split('"').next().unwrap()).collect()
}

/// Catches: a scene file that is not what the view shows, a page or snippet naming a file
/// the folder does not hold (a renamed viewer file, a stem escaped one way in the name and
/// another in the tag), and a scene name that breaks out of the page's HTML.
#[test]
fn the_folder_holds_what_the_view_shows_and_its_page_names_only_its_files() {
    let hostile = "my </pre><script>alert(1)</script> \"island\" & 'co'.vox";
    let mut checked = 0;
    for f in vxr_vox::fixtures::all() {
        // A world too wide to show as one map has no view to embed.
        let Ok(scene) = Scene::from_vox(&f.builder.build().write()) else { continue };
        let want = drawn(&scene.map, &scene.palette);
        let made = embed_files(&scene, hostile, b"poster".to_vec(), viewer());
        if want.is_empty() {
            assert_eq!(made.unwrap_err(), "the scene is empty", "{}", f.name);
            continue;
        }
        let files = made.unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(
            names,
            [
                "index.html",
                "my-pre-script-alert-1-script-island-co.vxrb",
                "my-pre-script-alert-1-script-island-co.png",
                "voxel-scene.js",
                "vxr_embed.js",
                "vxr_embed_bg.wasm"
            ],
            "{}",
            f.name
        );
        let (map, palette) = bricks::open(&files[1].1).unwrap();
        assert_eq!(drawn(&map, &palette), want, "{}: the scene file is what the view shows", f.name);
        assert_eq!(files[2].1, b"poster");

        let page = std::str::from_utf8(&files[0].1).unwrap();
        assert_eq!(srcs(page), [names[3], names[1], names[2]], "{}", f.name);
        // The loader and the file:// note are the page's only scripts, and the name is text.
        assert_eq!(page.matches("<script").count(), 2);
        assert_eq!(page.matches("<voxel-scene").count(), 1);
        assert!(page.contains(
            "alt=\"my &lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt; &quot;island&quot; &amp; &#39;co&#39;\""
        ));
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} fixtures had a view to embed");

    let snippet = embed_snippet(hostile);
    assert_eq!(
        srcs(&snippet),
        ["voxel-scene.js", "my-pre-script-alert-1-script-island-co.vxrb", "my-pre-script-alert-1-script-island-co.png"]
    );
    assert_eq!((snippet.matches("<script").count(), snippet.matches('<').count()), (1, 5));
}

/// Catches: a name made of nothing a file name keeps giving `.vxrb`, and a folder written
/// without one of the files the tag loads.
#[test]
fn an_unusable_name_and_a_missing_viewer_file() {
    for (name, stem) in [("", "scene"), ("../../.vox", "scene"), ("île 2.VOX", "île-2"), ("a.b.vox", "a-b")] {
        assert_eq!(embed_stem(name), stem, "{name:?}");
    }
    let mut scene = Scene::new([4, 4, 4]);
    scene.edit([([1, 1, 1], 3)]);
    let mut short = viewer();
    short.remove(1);
    assert_eq!(embed_files(&scene, "a.vox", Vec::new(), short).unwrap_err(), "the viewer file vxr_embed.js is missing");
}
