# Is there a market for an open-source voxel scene editor?

Market research, 2026-10-24. All numbers observed that day unless a date is given. Every claim carries a source link; figures marked "estimate" or "inference" are arithmetic on the cited numbers.

## Verdict

1. **Yes, but a small one.** Voxel-art communities run 10k-24k members each, against 2.7M for pixel art ([GummySearch r/VOXEL](https://gummysearch.com/r/VOXEL/), [r/PixelArt](https://gummysearch.com/r/PixelArt/)). itch.io has 1,169 voxel assets against 46,044 pixel-art assets ([voxel](https://itch.io/game-assets/tag-voxel), [pixel art](https://itch.io/game-assets/tag-pixel-art)). A loved open-source project is realistic. A business from the editor alone is not.
2. **The tool you liked, MagicaVoxel, is loved but stalled.** It has about 1.6M GitHub downloads. Its current build is Windows-only, the Mac build has been frozen at an Intel-only 0.99.6.2 since 2020, it has never had a Linux build, it is closed source, and each model is capped at 256^3. Nine of its ten most-upvoted issues are platform requests (Linux, Apple Silicon, black screens) ([releases](https://github.com/ephtracy/ephtracy.github.io/releases), [issues by reactions](https://github.com/ephtracy/ephtracy.github.io/issues?q=sort%3Areactions-%2B1-desc)).
3. **The strongest wedge is a browser-first editor with MagicaVoxel's feel, whose file format is also its embed format.** That means WebGPU with a WebGL2 fallback, sparse scenes well past 256^3, and a one-tag web embed. Nobody owns voxel embeds today: the only `.vox` web component, [`<vox-viewer>`](https://www.npmjs.com/package/vox-viewer), got 80 downloads last month ([npm API](https://api.npmjs.org/downloads/point/last-month/vox-viewer)).
4. **Win attention with scenes, not feature lists.** In 2026 the GitHub stars went to diorama builders: [Tiny World Builder](https://github.com/jasonkneen/tiny-world-builder) got 1,608 stars in 5.5 months. No voxel editor launched on HN since 2025 passed 53 points ([VoxCSS](https://news.ycombinator.com/item?id=46134541)).
5. **What v1 should be:** a fast web editor, a lossless `.vox` round-trip, a `<voxel-scene>` embed, and a headless/agent API. Leave collaboration, rigging and AI generation out of v1. If the project ever makes money, it will come from a hosted share-and-embed tier or platform sponsorship, not donations ([vengi Sponsors: $0](https://github.com/sponsors/mgerhardy), [Spline $25M raised](https://pulse2.com/spline-collaborative-3d-design-platform-company-secures-10-million-series-a)).

---

## What I did

- **Five research passes, run in parallel:**
  - desktop editors
  - web, WebGPU and AI tools
  - demand signals
  - business precedents and segments
  - technical gaps and risks

  Raw notes (about 170 KB) were consolidated into this report.
- **Sources and tools:**
  - built-in web search and fetch
  - the GitHub REST API through `gh-axi` (stars, releases, download counts, issues sorted by reactions)
  - the `hn.algolia.com` API for Hacker News stories and comments
  - Google Trends through `chrome-devtools-axi` (headless Chrome)
  - the iTunes Search API, Steam store pages and the SteamSpy API
  - the npm downloads API
  - Patreon and Open Collective JSON APIs
  - GummySearch for subreddit sizes
  - the PullPush archive for Reddit permalinks
- **No paid APIs were used.**
- **Blocked sources:**
  - Reddit refused this host (HTTP 403), so subreddit sizes are GummySearch's rounded figures and Reddit quotes come from the PullPush archive, with real permalinks.
  - Fiverr and ArtStation showed captchas, which I did not solve.
  - Sketchfab tag pages show no counts.
- **Example commands:**
  - `gh-axi api "repos/ephtracy/ephtracy.github.io/releases?per_page=100"`: download counts summed. All assets total 1,912,106; MagicaVoxel assets alone total 1,633,475.
  - `gh-axi api "search/issues?q=repo:ephtracy/ephtracy.github.io+is:issue&sort=reactions-%2B1"`: the top 10 are #30, #172, #21, #162, #197, #32, #307, #214, #67 and #276.
  - `gh-axi api "search/repositories?q=voxel+editor+created:>2025-01-01&sort=stars"`: 203 repos, the top one with 12 stars.
  - `https://hn.algolia.com/api/v1/search?query=magicavoxel&tags=comment`
- **Spot checks:** I re-ran the key GitHub numbers myself for Tiny World Builder, Goxel, vengi, Blockbench, Voxel Builder and the MagicaVoxel repo. They match.

---

## MagicaVoxel: the comparison point

The tool you liked was MagicaVoxel, a free, closed-source editor and path tracer by one anonymous developer ([site](https://ephtracy.github.io/index.html?page=mv_main)). A new tool has to match what people love about it on day one and beat it where it falls short.

### Why it is loved

| What users love | Evidence |
|---|---|
| Fast to learn, results in minutes | "you'll be buildinthings out of voxels in 20 minutes" ([HN](https://news.ycombinator.com/item?id=18510861)); "super intuitive ... a mix between minecraft, blender and photoshop" ([Reddit](https://www.reddit.com/r/ImaginarySliceOfLife/comments/kp0yv9/small_onsen/ghvfiez/)) |
| Hand-sculpting feel | "What I loved about MV was handsculpting everything" ([Reddit, 2026-07](https://www.reddit.com/r/Art/comments/1v3hmx0/isle_of_the_dead_hasan_goktepe_voxel_art_2021/oz9o2ez/)) |
| A beautiful path tracer built into the editor | "MagicaVoxel's renderer is just gorgeous" ([Reddit](https://www.reddit.com/r/MagicaVoxel/comments/lnmvsg/laterrrrz_rendered_at_15k_samples/go1o3ue/)); "renders those scenes very quickly" ([HN](https://news.ycombinator.com/item?id=11809650)) |
| Palette-first workflow: recolor the whole scene at once | [80.lv](https://80.lv/articles/using-voxels-for-simple-dioramas); batch HSV and palette swaps added in 0.99.7.1 ([changelog](https://github.com/ephtracy/ephtracy.github.io/blob/master/mv_main.html)) |
| Free, tiny, a single portable zip, no account | "it's total size is only a few mb" ([Reddit](https://www.reddit.com/r/VOXEL/comments/gq73jr/where_would_i_begin_learning_voxel_art/frr9y7x/)); "Free to use for any project" ([site](https://ephtracy.github.io/index.html?page=mv_main)) |
| Voxel shaders for procedural brushes | A Voxel Max reviewer misses "SHADERS ... foliage and other asset creation so much easier" ([App Store](https://apps.apple.com/us/app/voxel-max-3d-modeling/id1442352186)); the community shader repo has 679 stars ([repo](https://github.com/lachlanmcdonald/magicavoxel-shaders)) |
| `.vox` is the shared format | Spec repo has 1,382 stars ([voxel-model](https://github.com/ephtracy/voxel-model)); "Almost all assets used in Teardown was modeled in ... MagicaVoxel" ([Teardown](https://teardowngame.com/modding/)) |

### Where it falls short, and what a new tool would do instead

| Shortfall | Evidence | New tool |
|---|---|---|
| Mac frozen at 0.99.6.2 (Sep 2020, Intel only); never a Linux build | [#307](https://github.com/ephtracy/ephtracy.github.io/issues/307): "Is this project dead?", and "still waiting" as late as 2026-08. Rosetta 2 removal will stop the Mac build ([rosettacheck](https://rosettacheck.com/apps/EPH.MagicaVoxel)). Linux issues #30 (28 thumbs-up), #21 (24) and #162 (20) | Runs in any WebGPU browser, plus a native shell for Linux |
| 256^3 voxels per model; scene area about -1024..+1024 | "a nightmare. Stuff like terrain and large walls" ([Reddit, 2026-07](https://www.reddit.com/r/VOXEL/comments/1uqdl3a/i_find_the_256_limit_frustrating/owb38p9/)); [format issue #66](https://github.com/ephtracy/voxel-model/issues/66) | One sparse world with no per-object cap |
| 255-color palette, with material tied to palette slot | [.vox spec](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox.txt); [Teardown docs](https://teardowngame.com/modding/) | Keep the palette workflow, but allow multiple palettes and per-voxel materials |
| Undo capped at 100 commands; autosave freezes the app for seconds | [changelog](https://github.com/ephtracy/ephtracy.github.io/blob/master/mv_main.html); [issue #59](https://github.com/ephtracy/voxel-model/issues/59) | Unlimited undo and incremental background saves |
| No headless or batch rendering; rendering blocks editing | [#67](https://github.com/ephtracy/ephtracy.github.io/issues/67), open since 2018 (13 thumbs-up); "a separate window for rendering" ([Reddit](https://www.reddit.com/r/VOXEL/comments/760t5s/my_magicavoxel_wishlist/dobem1w/)) | CLI and API rendering; progressive path tracing that does not block editing |
| Frame-only animation that cannot be exported to glTF or FBX | "unusable for games because i can't export them" ([issue #8](https://github.com/ephtracy/voxel-model/issues/8)) | Frame animation exported to glTF (v2) |
| Closed source, no plugins, slow and quiet releases (2021-12, 2023-05, 2025-07) | "It frustrates me to no end that it's closed source" ([r/godot, score 50](https://www.reddit.com/r/godot/comments/1n6zv3j/the_open_source_gang/nc41us5/)); [changelog](https://github.com/ephtracy/ephtracy.github.io/blob/master/mv_main.html) | Open source with a scripting/plugin API |

Audience scale: MagicaVoxel assets on GitHub have 1,633,475 downloads ([releases API](https://api.github.com/repos/ephtracy/ephtracy.github.io/releases)). Its Patreon has 2,531 members, 385 of them paying ([Patreon](https://www.patreon.com/magicavoxel)).

---

## 1. Landscape

### 1a. Desktop editors

| Tool | License | Platforms | Price | Latest release / last commit | Traction | Strengths | Top complaints |
|---|---|---|---|---|---|---|---|
| [MagicaVoxel](https://ephtracy.github.io/) | Proprietary freeware | Win (current); Mac stuck at 0.99.6.2 (2020); no Linux | Free (Patreon) | 0.99.7.2, 2025-07-12 | 1.63M downloads; tracker repo 2,919 stars, 234 open issues | Speed, path tracer, palette, `.vox` standard | Mac/Linux, 256^3 cap, closed source, slow releases |
| [Goxel](https://github.com/guillaumechereau/goxel) | GPL-3.0 + commercial on request (CLA) | Win, Mac, Linux, BSD, iOS, Android | Desktop free; [iOS $4.99](https://apps.apple.com/us/app/goxel-3d-voxel-editor/id1259097826) (3.76 from 50 ratings) | v0.15.1, 2024-07-28 / 2026-07-26 | 3,213 stars; 134k downloads | Open source, unlimited sparse scene, layers | "UI just feels clunky" ([HN](https://news.ycombinator.com/item?id=46292054)); slows on big maps ([#414](https://github.com/guillaumechereau/goxel/issues/414)) |
| [vengi voxedit](https://github.com/vengi-voxel/vengi) | MIT (some assets CC BY-SA) | Win, Mac, Linux, web (older build) | Free | v0.6.0, 2026-09-19 / 2026-10-22 | 1,433 stars; 77k downloads | About 70 formats, Lua, animation with IK, path tracer, network collaboration, [MCP server](https://github.com/vengi-voxel/vengi/blob/master/docs/voxedit/MCP.md) | Small audience, dense UI, editing lag report ([#820](https://github.com/vengi-voxel/vengi/issues/820)) |
| [Blockbench](https://github.com/JannisX11/blockbench) | GPL-3.0 | Win, Mac, Linux, web | Free | v5.2.2, 2026-10-08 / 2026-10-18 | 6,091 stars; about 1M downloads per release; official [Hytale](https://hytale.com/news/2025/11/hytale-modding-strategy-and-status) tool | Minecraft and Hytale audience, 147 plugins, animation | Cuboid/low-poly modeler, not a voxel grid; 651 open issues |
| [Avoyd](https://www.avoyd.com/) | Proprietary | Windows only | Free demo; [Indie $22.50](https://enkisoftware.itch.io/avoyd) | 0.27.3, 2026-08-22; 0.28 beta 2026-10-16 | 1 itch rating; Patreon $6.44/month ([API](https://www.patreon.com/api/campaigns/2613712)) | Worlds up to 262,144 a side, 65,535 materials ([FAQ](https://www.avoyd.com/faq.html)) | Windows only; no animation or shaders (its own FAQ) |
| [Qubicle](https://www.getqubicle.com/) | Proprietary | Win, Mac | [Steam $14.99](https://store.steampowered.com/app/454550/) + paid modules | 3.1.2, 2017; "no longer in active development" | Steam Mixed, 63% of 121 reviews | Pro mesh export (Crossy Road) | Basic features locked behind paid add-ons; broken `.vox` |
| [Foxel](https://foxel.io/) (Qubicle successor) | Proprietary | Windows 10+ | Not yet published; 30-day trial | New in 2026 | No reviews yet | PBR, prefabs, animation, Lua, CLI | Windows only; unsigned installer ([download](https://foxel.io/download.html)) |
| VoxEdit (The Sandbox) | Proprietary freeware | Win, Mac | Free | No public version ([FileHorse](https://www.filehorse.com/download-voxedit/)) | Tied to The Sandbox | Model, rig and animate | Animations export only to the Sandbox marketplace ([GameFromScratch](https://gamefromscratch.com/?p=24197)); NFT tie-in |
| [Voxel Max](https://voxelmax.com/) | Proprietary | iPhone, iPad, Mac | [$9.99](https://apps.apple.com/us/app/voxel-max-3d-modeling/id1442352186) | 1.5.56, 2026-10-22 | 4.77 from 39 ratings | Polish, Pencil, image-to-3D | Apple only; data-loss reviews |
| [VoxelShop](https://github.com/simlu/voxelshop) | Apache-2.0 | Java desktop | Free | 1.8.26, 2019 | 366 stars | Layers, many exports | Abandoned in 2019 |
| [FileToVox](https://github.com/Zarbuz/FileToVox) | MIT | Windows | Free | 1.16, 2022 / 2023 | 523 stars | Converts meshes, images and point clouds to `.vox` | Dormant; exists to work around MagicaVoxel's limits |
| [RPG in a Box](https://store.steampowered.com/app/498310/RPG_in_a_Box/) | Proprietary | Win, Mac, Linux | $19.99 | Active | Very Positive, 93% of 433 | Voxel model maker inside a game engine | Tied to its own engine |
| [SpriteStack](https://store.steampowered.com/app/1106180/SpriteStack/) | Proprietary | Win, Mac, web | $29.99 | Active | Mixed, 53% of 80 | Sprite-stacked voxel art | Mixed reviews |

Small or new: [Roxel](https://github.com/starzonmyarmz/roxel) (MIT, Rust/Bevy, 3 stars), [VoxelDream](https://stylophone.itch.io/voxeldream) (free Mac beta), [Mega Voxels](https://apps.apple.com/us/app/mega-voxels/id1539984942) (iOS, 4.79 from 1,824 ratings, the most-rated mobile voxel editor), [VoxelForge](https://store.steampowered.com/app/4790610/VoxelForge/) (Steam, 1 review).

### 1b. Web and WebGPU tools

| Tool | Type | License | Price | Recency | Traction | Note |
|---|---|---|---|---|---|---|
| [Voxel Builder](https://github.com/nimadez/voxel-builder) | Web voxel modeler | GPL-3.0 | Free | 4.8.5, 2026-09-09 | 165 stars, the most-starred browser-first voxel modeler | README: "you shouldn't expect much", "Low FPS at higher voxel volumes" ([known issues](https://github.com/nimadez/voxel-builder#known-issues)) |
| [Tiny World Builder](https://github.com/jasonkneen/tiny-world-builder) | Browser diorama builder | AGPL-3.0 | Free, plus a $TINYWORLD token | Created 2026-05-10, active | 1,608 stars; [Product Hunt #32, 78 upvotes](https://hunted.space/product/tiny-world-builder) | Tile kit, not freeform voxels; the token may inflate attention |
| [Mykonos Island Voxels](https://github.com/boona13/mykonos-island-voxels) | Isometric voxel toy | MIT | Free | A one-day build, 2026-05-14 | 1,061 stars | 14x14 grid, no export |
| [VoxCSS / PolyCSS](https://github.com/layoutit/polycss) | DOM/CSS voxel engine + editor | MIT | Free | 2026-08 | 771 stars; [HN 53 pts](https://news.ycombinator.com/item?id=46134541) | About 40 kB, small scenes only |
| [Heerich.js](https://github.com/meodai/heerich) | Code-to-SVG voxel scenes | MIT | Free | 2026-06 | 628 stars | Not an editor |
| [VoxelDraft](https://hunted.space/product/voxeldraft) | Web editor | No license | Free | 2026-05 | 0 Product Hunt upvotes | 64^3 cap |
| [VoxiGen](https://discourse.threejs.org/t/voxigen-ai-powered-browser-voxel-editor-generate-edit-3d-models-from-text-images/89926) | Web editor + AI | Proprietary | Free | 2026-02 | "200+ creators" (self-reported) | Exports were "coming soon" at launch |
| [vox-viewer](https://github.com/FlorianFe/vox-viewer) | `.vox` web component | MIT | Free | 2025-03 | 27 stars, 80 npm downloads/month | The only dedicated voxel embed |
| [SuperSplat](https://github.com/playcanvas/supersplat) | Gaussian splat editor | MIT | Free | Active | 10,450 stars | Uses voxels for collision ([docs](https://developer.playcanvas.com/user-manual/splat-transform/collision/)); a complement, not a rival |
| [Spline](https://spline.design/pricing) | Hosted 3D design + embeds (not voxel) | Proprietary | Free with watermark; $15-70 per seat per month | Active | $25M raised; runtime 394k npm downloads/week ([npm](https://www.npmjs.com/package/@splinetool/runtime)) | The precedent for a hosted embed business |

WebGPU voxel renderers ([three-voxel-destruction](https://github.com/sbobyn/three-voxel-destruction), [webgpu-voxel-raymarching](https://github.com/jamestkiernan/webgpu-voxel-raymarching)) are demos with fewer than 20 stars each. [Douglas Dwyer's Octo](https://github.com/DouglasDwyer/octo-release) (Rust, WASM, WebGPU) froze its public repo in Aug 2024.

### 1c. AI text-to-voxel and image-to-voxel

| Tool | What it is | Price | Traction | Weakness |
|---|---|---|---|---|
| [Meshy](https://www.meshy.ai/features) | Text/image-to-3D with a "Voxel" style preset | Free tier, credits | $100M ARR, 15M+ users ([Dealroom](https://dealroom.co/news/158079-meshy-hits-100m-arr-first-ai-3d-firm-to-reach-milestone/)) | Removed its `text-to-voxel` API on 2025-10-01 ([changelog](https://docs.meshy.ai/en/api/changelog)); output is a mesh, not an editable grid |
| [Tripo](https://www.tripo3d.ai/blog/how-to-achieve-different-artistic-styles) | 3D generation with voxel/Lego stylization | From $12/month | 20M users ([Implicator](https://www.implicator.ai/vast-raises-nearly-200-million-for-tripo-ai-3d-models/)) | Style filter, not an editor |
| [VoxelAI Studio](https://voxelai.studio) | Browser editor + text-to-voxel | $8-49/month | Not disclosed | Small token allowances |
| [Sorceress Voxel Studio](https://sorceress.games/pages/voxel-studio) | Text/image to rigged voxel characters | $49 lifetime | Not disclosed | No traction data |
| [TRELLIS.2](https://huggingface.co/microsoft/TRELLIS.2-4B) | Open image-to-3D model built on sparse voxels | Free (MIT) | 12,425 stars ([repo](https://github.com/microsoft/TRELLIS.2)) | Outputs PBR meshes, not palette art |
| [MineBench](https://github.com/Ammaar-Alam/minebench) / [VoxelBench](https://benchlm.ai/benchmarks/voxelbench) | LLM voxel-build benchmarks | Free | MineBench 341 stars; VoxelBench ranks 60 models | Output is throwaway JSON or three.js |

The viral "voxel pagoda garden" prompt is now an informal LLM benchmark ([X/Grok](https://x.com/grok/status/2062774888054563080)). The complaint about it is editability: you "can't just change the 'hat' of the Pelican" ([HN](https://news.ycombinator.com/item?id=49590523)). About eight MagicaVoxel MCP repos exist; the largest has 28 stars ([magicavoxel-mcp](https://github.com/Mahinika/magicavoxel-mcp)). The [Blockbench MCP plugin](https://github.com/jasonjgardner/blockbench-mcp-plugin) has 562 stars.

---

## 2. Demand signals

### 2a. Community sizes

| Community | Size | Growth per year | Source |
|---|---|---|---|
| r/VoxelGameDev | 24k | +32% | [GummySearch](https://gummysearch.com/r/VoxelGameDev) |
| r/VOXEL | 23k | +22% | [GummySearch](https://gummysearch.com/r/VOXEL/) |
| r/MagicaVoxel | 10k | +13% | [GummySearch](https://gummysearch.com/r/MagicaVoxel/) |
| r/Blockbench | 35k | +64% | [GummySearch](https://gummysearch.com/r/Blockbench/) |
| r/aseprite (pixel art tool) | 72k | +58% | [GummySearch](https://gummysearch.com/r/aseprite/) |
| r/PixelArt | 2.7M | +3% | [GummySearch](https://gummysearch.com/r/PixelArt/) |
| r/aigamedev | 90k | +467% | [GummySearch](https://gummysearch.com/r/MagicaVoxel/) |
| MagicaVoxel Patreon | 2,531 members, 385 paying | - | [Patreon](https://www.patreon.com/magicavoxel) |

### 2b. Hacker News

| Story | Points | Date |
|---|---|---|
| [Blockbench](https://news.ycombinator.com/item?id=33845291) | 340 | 2022-12 |
| [MagicaVoxel](https://news.ycombinator.com/item?id=38050106) | 334 | 2023-10 |
| [SpriteStack voxel editor](https://news.ycombinator.com/item?id=27783490) | 315 | 2021-07 |
| [MagicaVoxel](https://news.ycombinator.com/item?id=18721555) | 293 | 2018-12 |
| [A voxel Tokyo in real Japan time](https://news.ycombinator.com/item?id=48890959) | 381 | 2026-07 |
| [Voxile ray-traced voxel game](https://news.ycombinator.com/item?id=47239042) | 279 | 2026-03 |
| [Show HN: VoxCSS](https://news.ycombinator.com/item?id=46134541) | 53 | 2025-12 |
| [Show HN: Cube Art Project 2 multiplayer browser editor](https://news.ycombinator.com/item?id=45481364) | 3 | 2025-10 |

A polished voxel editor reaches the HN front page at about 300 points. New editors launched since 2025 have not; the highest scorers since then are visual experiences and engines.

### 2c. itch.io and Steam

| Metric | Value | Source |
|---|---|---|
| itch.io voxel assets (484 free) | 1,169 | [itch](https://itch.io/game-assets/tag-voxel) |
| itch.io pixel-art assets | 46,044 | [itch](https://itch.io/game-assets/tag-pixel-art) |
| itch.io voxel tools vs pixel-art tools | 131 vs 3,024 | [voxel](https://itch.io/tools/tag-voxel), [pixel art](https://itch.io/tools/tag-pixel-art) |
| Paid voxel tool prices on itch | $3.50-$30 (SpriteStack $30, Avoyd $22.50, AniVoxel $20) | [top sellers](https://itch.io/tools/top-sellers/tag-voxel) |
| Steam games tagged Voxel vs Pixel Graphics | 2,085 vs 49,445 | [Voxel](https://store.steampowered.com/search/?tags=1732), [Pixel](https://store.steampowered.com/search/results/?tags=3964&infinite=1) |
| Teardown | 2.5M players; 14,680 Workshop items, made in MagicaVoxel | [Wikipedia](https://en.wikipedia.org/wiki/Teardown_(video_game)), [Workshop](https://steamcommunity.com/workshop/browse/?appid=1167630&browsesort=trend&section=readytouseitems&days=-1) |
| Hytale | Early access 2026-01-13, passed 1M players; its official modeling tool is Blockbench | [Wikipedia](https://en.wikipedia.org/wiki/Hytale), [Hytale blog](https://hytale.com/news/2025/11/hytale-modding-strategy-and-status) |

### 2d. Google Trends

<!-- TRENDS_CHART -->

Yearly average of the Google Trends index, worldwide, past 5 years, all four terms on one shared scale ([query](https://trends.google.com/trends/explore?date=today%205-y&q=magicavoxel,blockbench,goxel,aseprite)). 2021 starts on Oct 24; 2026 runs to Oct 18.

| Year | blockbench | aseprite | magicavoxel | goxel |
|---|---|---|---|---|
| 2021 | 18.6 | 18.9 | 6.5 | 0.2 |
| 2022 | 32.5 | 30.5 | 7.6 | 0.6 |
| 2023 | 36.3 | 33.2 | 4.3 | 0.5 |
| 2024 | 44.6 | 44.1 | 3.7 | 0.4 |
| 2025 | 60.6 | 53.1 | 4.0 | 0.5 |
| 2026 | 82.3 | 61.8 | 4.5 | 0.7 |

What the trends show:
- **MagicaVoxel peaked during the NFT boom.** Its all-time peak was Jan 2022, and it has been flat at about half that since 2023 ([query](https://trends.google.com/trends/explore?date=all&q=magicavoxel)).
- **The growth went to Blockbench and Aseprite.** Both are driven by modding and indie game dev, and Blockbench now gets about 18 times MagicaVoxel's search interest.
- **Rising related queries:** "magicavoxel animation", "voxel art maker" (+40%), "blockbench hytale" and "aseprite mcp" (+1,400%).
- **The 2026 rise in generic "voxel" searches is noise.** It comes from crypto tokens, not art.

### 2e. Commissions and the NFT bust

- **Commissions are cheap and pay little.** Fiverr voxel artists start at about $10/hr ([pentakie](https://www.fiverr.com/pentakie)), and some gigs still target "nft, the sandbox" ([gig](https://fiverr.com/eugenelulu/make-a-voxel-model)).
- **The NFT-era demand has collapsed.** The Sandbox's token is down about 97% from its peak, and the company cut at least half its staff in Aug 2025 ([Cointelegraph](https://cointelegraph.com/news/the-sandbox-restructuring-ai-layoffs-cofounders-role-shift)). Property in Voxels (formerly Cryptovoxels) lost over 90% of its value ([Technext](https://technext24.com/?p=194836)).
- **Tutorial audiences are small.** The top MagicaVoxel tutorials have 50k-275k views ([PremiumBeat](https://www.youtube.com/watch?v=uKOBIHSgIwI)), against 1.1M for a Blockbench beginner video ([ArtsByKev](https://www.youtube.com/watch?v=dsax5p4brN8)).

### 2f. User pain points, in their words

- [MagicaVoxel] "I find the 256 x 256 x 256 voxels limit for a single object really frustrating. I have a hard time working with multiple objects" - [r/VOXEL, 2026-07-08](https://www.reddit.com/r/VOXEL/comments/1uqdl3a/i_find_the_256_limit_frustrating/)
- [MagicaVoxel] "Having to split up your scene into multiple blocks that are different to your actual objects is a nightmare. Stuff like terrain and large walls." - [r/VOXEL, 2026-07-08](https://www.reddit.com/r/VOXEL/comments/1uqdl3a/i_find_the_256_limit_frustrating/owb38p9/)
- [MagicaVoxel] "With the highest of respect, I wish this project was handed over to more active maintainers, such potential." - [GitHub #307](https://github.com/ephtracy/ephtracy.github.io/issues/307#issuecomment-1783395180)
- [MagicaVoxel] "It really is a shame that not only there's no longer support for I don't know how many years now, but also no answers." - [GitHub #307, 2025-08-28](https://github.com/ephtracy/ephtracy.github.io/issues/307#issuecomment-3232425798)
- [MagicaVoxel] "It frustrates me to no end that it's closed source because it's part of the art pipeline at my job ... If it were open source, or at least allowed you to implement add-ons, it would've been so much better for serious production" - [r/godot, score 50, 2025-09-03](https://www.reddit.com/r/godot/comments/1n6zv3j/the_open_source_gang/nc41us5/)
- [MagicaVoxel] "Being able to specify rendering options on the command line would make MV a million times more useful." - [GitHub #67](https://github.com/ephtracy/ephtracy.github.io/issues/67)
- [MagicaVoxel] "Frame-by-frame animations are good but unusable for games because i can't export them to anything usable." - [voxel-model #8](https://github.com/ephtracy/voxel-model/issues/8)
- [MagicaVoxel] "I'd also like to be able to view the rendering in its own window - as opposed to switching back and forth between rendering and editing." - [r/VOXEL](https://www.reddit.com/r/VOXEL/comments/760t5s/my_magicavoxel_wishlist/dobec2h/)
- [Several tools] "I'm starting a new voxel game so I'm looking for an editor that can last a few years. But looking at the most popular ones they're all abandoned." - [r/VoxelGameDev](https://www.reddit.com/r/VoxelGameDev/comments/lz832x/any_voxel_editor_that_is_not_abandoned/)
- [Goxel] "I really don't like Goxel. It's UI just feels clunky." - [HN, 2025-12-16](https://news.ycombinator.com/item?id=46292054)
- [Goxel] "at a certain point, performance suddenly takes a nosedive ... it's one of the major complaints I get about working with Goxel" - [GitHub #414](https://github.com/guillaumechereau/goxel/issues/414)
- [VoxEdit] "every time I selected more than a single piece it would lag to the point of crashing ... I lost a ton of work myself." - [r/godot](https://www.reddit.com/r/godot/comments/fxtv0r/me_an_completly_coding_newbie_trying_to_make_an/fmy1bae/)
- [Qubicle] "basic features like 3D box selection, OBJ export ... locked behind DLC" - [Steam reviews](https://steamcommunity.com/app/454550/negativereviews/?browsefilter=toprated)
- [Web editors] "Note that this application runs on a web browser and you shouldn't expect much." - [Voxel Builder README](https://github.com/nimadez/voxel-builder#known-issues)

---

## 3. Who would use it

| Segment | Rough size | Willingness to pay | Fit for a new tool | Source |
|---|---|---|---|---|
| Voxel artists and hobbyists | Voxel subreddits about 60k-70k in total; MagicaVoxel 1.6M lifetime downloads, 2,531 patrons. Estimate: tens of thousands active | Low; free tools set the price at $0 | **Core.** MagicaVoxel refugees on Mac and Linux, and anyone hitting the 256^3 cap | [GummySearch](https://gummysearch.com/r/VOXEL/), [Patreon](https://www.patreon.com/magicavoxel) |
| Indie game devs using voxel art | 2,085 voxel-tagged Steam games; 3,475 on itch; r/VoxelGameDev 24k; about 19k Steam releases in 2025 | Medium; they pay $3.50-$30 for tools | **Core.** They need engine export, glTF and an editor that will not be abandoned | [Steam](https://store.steampowered.com/search/?tags=1732), [itch](https://itch.io/games/tag-voxel), [GamingOnLinux](https://www.gamingonlinux.com/2025/12/over-19000-games-have-released-on-steam-in-2025-with-nearly-half-seeing-fewer-than-10-reviews/page=1/) |
| Teardown and other voxel-game modders | Teardown: 2.5M players, 14,680 Workshop items | Low | Strong; they work in `.vox` at large scale | [Workshop](https://steamcommunity.com/workshop/browse/?appid=1167630&browsesort=trend&section=readytouseitems&days=-1) |
| Web developers and designers who want a 3D scene on a site | three.js 15.2M npm downloads/week; model-viewer 2.1M/month; Spline runtime 394k/week | Medium-high for hosted tiers, near zero for donations | **The wedge.** No voxel embed exists. It is also what prompted this research: a voxel desk scene for a homepage | [three](https://www.npmjs.com/package/three), [Spline pricing](https://spline.design/pricing) |
| AI agents and LLM tinkerers | MineBench 341 stars; VoxelBench ranks 60 models; r/aigamedev 90k (+467%/yr) | Unproven | Strong as a channel: an editable target for LLM output | [MineBench](https://github.com/Ammaar-Alam/minebench), [VoxelBench](https://benchlm.ai/benchmarks/voxelbench) |
| Minecraft and Hytale creators | Minecraft about 170M MAU (2022 official); Hytale 1M+; Modrinth 70,863 authors | High for content ($500M+ Marketplace payouts), low for tools | Weak. Blockbench owns entity modeling; only scene and prefab building is open | [Windows Central](https://www.windowscentral.com/minecraft-microsoft-fy21-q3-report), [Modrinth](https://api.modrinth.com/v2/statistics), [corq](https://corq.studio/insights/minecraft-marketplace-inside-the-platforms-500-million-creator-economy/) |
| Educators | Minecraft Education reported at 35M users (secondary) | Institutional, about $5/seat/year | Later. A browser tool with no install suits classrooms | [El Universal](https://www.eluniversal.com.mx/techbit/aprendizaje-con-videojuegos/), [pricing](https://edusupport.minecraft.net/hc/en-us/articles/360047119092-FAQ-Availability-Pricing-and-Licensing) |
| NFT and metaverse creators | The Sandbox 6.3M wallet accounts, now in retreat | Collapsing | Avoid | [CoinDesk](https://www.coindesk.com/consensus-hong-kong-2025-coverage/2025/01/28/the-sandbox-s-sebastien-borget-on-the-future-of-web3-gaming) |

---

## 4. Gaps a new tool could own

Ranked by strength of evidence.

| Gap | Evidence | Who covers it today | Verdict |
|---|---|---|---|
| **In-browser, runs anywhere** | Platform requests make up 175 of the 188 thumbs-up on MagicaVoxel's top 10 issues ([tracker](https://github.com/ephtracy/ephtracy.github.io/issues?q=sort%3Areactions-%2B1-desc)). WebGPU ships in Chrome/Edge 113+, Safari 26 and Firefox 141+ on Windows, with 87.82% global support ([caniuse](https://caniuse.com/webgpu), [gpuweb status](https://github.com/gpuweb/gpuweb/wiki/Implementation-Status)) | Blockbench (not voxel); Voxel Builder (slow); vengi's web build is labeled not current | **Own it.** Ship a WebGL2 fallback or native shell for Linux, where WebGPU is still patchy |
| **Performance at scale** | The 256^3 cap is the most-quoted 2026 complaint. Editable compressed voxels are solved in research: HashDAG ([TU Delft](https://research.tudelft.nl/en/publications/interactively-modifying-compressed-sparse-voxel-representations)), brickmaps ([Utrecht](https://studenttheses.uu.nl/handle/20.500.12932/20460)). Teardown ray-traces volume textures ([acko.net](https://acko.net/blog/teardown-frame-teardown/)) | Avoyd only (paid, Windows); Goxel slows down | **Own it.** No open-source, artist-friendly tool does this, and none does it in a browser |
| **Embeddable web export** | `<vox-viewer>` gets 80 downloads/month against 2.1M for model-viewer ([npm](https://api.npmjs.org/downloads/point/last-month/vox-viewer)). Sketchfab does not take `.vox` ([guide](https://sketchfab.com/blogs/community/complete-guide-importing-magicavoxel-sketchfab)). Exported meshes are bloated ([Goxel #315](https://github.com/guillaumechereau/goxel/issues/315)) | Nobody | **Own it.** The file is the embed, and every embed advertises the tool |
| **Format interop** | `.vox` v200 changes are undocumented ([#264](https://github.com/ephtracy/ephtracy.github.io/issues/264)). Goxel's most-commented issues are about formats ([#57](https://github.com/guillaumechereau/goxel/issues/57)). The OpenVoxelFileFormat effort is stalled at the requirements stage with 4 stars ([repo](https://github.com/enkisoftware/OpenVoxelFileFormat)) | vengi (about 55 voxel formats) | Lossless `.vox` plus glTF is required. Publish an open scene spec, and consider vengi's converter for the long tail |
| **Procedural and scripting** | MagicaVoxel #67 and #92 (headless render); Goxel #22; MagicaVoxel shaders are loved | vengi (Lua), MagicaVoxel (shaders) | v1: CLI and JS API. v2: brush scripts |
| **AI assist** | LLM voxel benchmarks are popular, but their output cannot be edited ([HN](https://news.ycombinator.com/item?id=49590523)). Blockbench MCP has 562 stars. Meshy dropped its voxel API | vengi MCP; small MCP repos | Cheap to add: an MCP/API that writes editable scenes, plus mesh-to-voxel import. Do not build a generator |
| **Animation** | Strong in Blockbench's tracker (rigging 25, IK 10); weak in voxel-only trackers. Animation tools sell on itch (AniVoxel $20) | vengi, VoxEdit, Blockbench | v2: frame animation with glTF export. Skip rigging |
| **Collaboration** | No tracker asks for it. vengi and Blockbench already ship session-based collaboration ([vengi network](https://github.com/vengi-voxel/vengi/blob/master/docs/voxedit/usage/Network.md)) | vengi, Blockbench | Later. Link sharing comes free with the web wedge; real-time editing is a bet |

---

## 5. Open-source business viability

| Precedent | Model | Numbers | Source |
|---|---|---|---|
| MagicaVoxel | Free, closed source, Patreon | 385 paying patrons; estimated $511-$3K/month | [Graphtreon](https://graphtreon.com/creator/magicavoxel) |
| vengi | MIT, donations | $0/month; GitHub Sponsors goal at 0% | [Sponsors](https://github.com/sponsors/mgerhardy) |
| Goxel | GPL-3.0 + CLA, paid mobile apps | iOS $4.99 (50 ratings), Android $3.49 (1K+ downloads); no revenue disclosed | [App Store](https://apps.apple.com/us/app/goxel-3d-voxel-editor/id1259097826), [Play](https://play.google.com/store/apps/details?id=com.noctuasoftware.goxel) |
| Blockbench | GPL-3.0, PayPal donations, Mojang support, Hytale endorsement | About 1M downloads per release; no public income figure | [FAQ](https://github.com/JannisX11/blockbench.net/blob/main/content/blockbench/faq.md), [Hytale](https://hytale.com/news/2025/11/hytale-modding-strategy-and-status) |
| Aseprite (closest analog) | GPL until 2016, then source-available; $19.99 binary | 21,818 Steam reviews; about 1.43M owners (SteamSpy); roughly $13M-$29M lifetime gross at list price (estimate) | [SteamSpy](https://steamspy.com/api.php?request=appdetails&appid=431730) |
| LibreSprite | GPL fork of Aseprite, no funding | 1.43M downloads, $0 | [repo](https://github.com/LibreSprite/LibreSprite) |
| Pixelorama | MIT, $9.99 on Steam, Patreon | 85 Steam reviews (about 2.5k-5k units, estimate); Patreon $156/month | [Steam](https://store.steampowered.com/app/2779170/), [Patreon API](https://www.patreon.com/api/campaigns/3639501) |
| Krita | GPL, paid Steam/Store builds + dev fund | Fund EUR 2,477/month; Steam 200k-500k owners | [fund](https://fund.krita.org/), [SteamSpy](https://steamspy.com/api.php?request=appdetails&appid=280680) |
| Blender | Foundation and dev fund | EUR 283,795/month; 2025 income EUR 5.71M on 22M downloads | [fund](https://fund.blender.org/), [2025 report](https://download.blender.org/foundation/Blender-Foundation-Annual-Report-2025.pdf) |
| Qubicle | Paid binary plus paid modules | About 3.6k-7.3k sales in 10 years (estimate); now discontinued | [Steam](https://store.steampowered.com/app/454550/) |
| Spline | Hosted 3D design + embeds, closed source | $25M raised; $15-70 per seat per month | [pulse2](https://pulse2.com/spline-collaborative-3d-design-platform-company-secures-10-million-series-a), [pricing](https://spline.design/pricing) |
| tldraw | Left permissive licensing for SDK licences | Just under $1M in first-year licence deals; $10M Series A | [blog](https://tldraw.dev/blog/announcing-tldraw-series-a) |
| Excalidraw+ | MIT editor + $6/user/month SaaS | Open Collective about $2.2k/year; SaaS revenue not public | [Open Collective](https://opencollective.com/excalidraw), [pricing](https://plus.excalidraw.com/pricing) |
| Sketchfab, PlayCanvas | Embed/viewer and hosted editor | Exited by acquisition (Epic 2021, then KitBash 2026; Snap 2017) | [CG Channel](https://www.cgchannel.com/2021/07/epic-games-buys-sketchfab/), [Digital Production](https://digitalproduction.com/2026/08/12/kitbash-buys-artstation-and-sketchfab/), [Game Developer](https://gamedeveloper.com/game-platforms/snapchat-acquires-cloud-based-engine-playcanvas) |
| three.js | MIT library | 15.2M npm downloads/week; 29 public sponsors | [npm](https://www.npmjs.com/package/three), [Sponsors](https://github.com/sponsors/mrdoob) |

What the precedents say:
1. **Donations do not fund a voxel tool.** Every voxel precedent above earns $0 to about $3K a month.
2. **A paid convenience build with the source open (Aseprite, Krita) works only for the category default.** Followers sell a few thousand units at most.
3. **Platform anchoring keeps Blockbench alive.** Mojang supports it and Hytale endorses it.
4. **The only voxel-adjacent route with venture-scale precedents is a hosted editor with an embed runtime (Spline).** Those businesses tend to end in acquisition.

For this project, model 4 maps onto the embed wedge: the editor and runtime are free and open, and a hosted tier charges for scene hosting, share links, no watermark and team seats. That tier is optional. The project can succeed as open source without it.

---

## 6. Risks

| Risk | Severity | Evidence | Mitigation |
|---|---|---|---|
| MagicaVoxel is free, loved and good enough | High | 1.6M downloads; Teardown's pipeline depends on it ([Teardown](https://teardowngame.com/modding/)) | Lossless `.vox` import and export on day one; match its feel; win on platforms, scale and embeds |
| vengi already ships most features | High | MIT; collaboration, Lua, MCP, IK, path tracer, a release every 1-5 months ([features](https://github.com/vengi-voxel/vengi/blob/master/docs/voxedit/Features.md)) | Compete on feel, web-first design and the embed; do not rebuild its format zoo |
| Blockbench owns Minecraft and Hytale | High in that niche | Official Hytale tool; Google Trends interest up 4.4x in 5 years | Stay out of cuboid entity modeling; focus on voxel scenes |
| Small market, $0 price anchor | High | Section 2 and Section 5 numbers | Plan it as a project, not a company; keep the hosted tier optional |
| Solo-maintainer burnout | High | Goxel 2,525 commits vs 13 for the next contributor; vengi 23,779 vs 82 ([repos](https://github.com/vengi-voxel/vengi/graphs/contributors)); Qubicle went dark | Small modular core, a published format spec, contributor docs from day one |
| AI 3D makes hand-made voxel art less necessary | Medium | Meshy and Tripo voxel styles; TRELLIS.2 is built on sparse voxels | Become where AI output gets cleaned up and edited (mesh-to-voxel, palette reduction, MCP) |
| WebGPU fragmentation | Medium | Linux and Firefox Linux/Android gaps ([gpuweb](https://github.com/gpuweb/gpuweb/wiki/Implementation-Status)); 256 MiB default buffer and 4 GB wasm32 limits ([W3C](https://www.w3.org/TR/webgpu/), [SpiderMonkey](https://spidermonkey.dev/blog/2025/01/15/is-memory64-actually-worth-using.html)) | Sparse bricks, a WebGL2 fallback, a native shell on the same core |
| Fad cycles | Medium | MagicaVoxel search interest halved after the NFT peak ([Trends](https://trends.google.com/trends/explore?date=all&q=magicavoxel)) | Do not tie the tool to a token or platform economy |
| New commercial entrants | Low-medium | Foxel (Qubicle's successor), VoxiGen, VoxelAI Studio | Open source plus the web make the moat; none of them is both |

---

## 7. Verdict and recommendation

**Is there a market?** Yes, a real but niche one. Its edges are growing: AI game dev, web 3D and modding. Demand is proven by MagicaVoxel's 1.6M downloads and years of unanswered platform and size requests. Paid demand for the editor itself is near zero. Build it because it can become the open default for voxel scenes on the web, not because the editor will pay.

**Strongest wedge:** "MagicaVoxel's feel, in any browser, with no size cap, and every scene is a one-line embed." Three things make the wedge work:
- It answers the top demand class (platforms) by design.
- It removes the most-quoted current complaint (256^3).
- It enters a category nobody owns (voxel embeds), and every embed spreads the tool.

The trigger for this research, a voxel desk scene for a homepage, is exactly the embed use case.

**What v1 should be**
1. An editor with MagicaVoxel parity on the core loop: add, erase, paint, box and face tools, mirror, a palette-first workflow, and keyboard-first controls, all at 60 fps.
2. One sparse scene with no per-object cap: bricks of 8^3-16^3 in a pooled GPU buffer, ray-marched on WebGPU, with a WebGL2 fallback (see the scale and browser evidence in Section 4).
3. A progressive path-traced preview that does not block editing.
4. Lossless `.vox` import and export (including multi-model scenes and v200), plus glTF export with greedy meshing.
5. A `<voxel-scene src="scene.vxs">` web component that streams the native file, plus a static share-page export.
6. A CLI and JS API, which also covers headless rendering, plus an MCP server so agents can write editable scenes.

**Not in v1:** real-time collaboration, rigging, AI generation, a plugin marketplace and a hosted tier.

**Suggested sequence, in agent wall-clock time**
1. **Throwaway demo, about 1-2 hours.** Show a WebGPU viewer rendering a 512^3+ scene and the embed tag, with the homepage desk scene as the test content. It checks the look, the feel and the performance before any real build.
2. **Editor v1, roughly 1-3 days of agent wall-clock** across several sessions, counting review and CI.
3. **Launch with a striking scene** in the HN Show HN / r/VOXEL / r/threejs channels. A pretty scene earns stars; a feature list does not.

### Open choices

- **Go or no-go** on building the project.
- **Platform:** web-first with a native shell (recommended), or native-first.
- **License:**
  - (a) Permissive MIT/Apache-2.0 for everything, for maximum adoption (recommended for the format spec and embed runtime at least).
  - (b) MPL-2.0 or GPL-3.0 with a CLA on the editor, to keep paid builds or a hosted tier possible.

---

## Appendix: method caveats

- Steam unit estimates use 30-60 sales per review and are accurate to about +/-30-50% ([impress.games](https://impress.games/blog/steam-benchmarks-and-estimates-analysis)).
- Subreddit counts are GummySearch's rounded figures; r/VoxelArt's data is from Jan 2025.
- MagicaVoxel's Patreon hides earnings, so the $511-$3K/month figure is a Graphtreon estimate.
- Minecraft MAU above 170M is not an official figure. The Minecraft Education 35M and Marketplace $500M+ figures come from secondary sources.
- Two claims could not be verified: that the Voxels servers were shut down, and "Blockhammer" (seen in one 2026 listicle). Neither is used in the conclusions.
