# AI in vxr: bring-your-own-key assistant and an MCP server

Desk research, 2026-10-25. No API calls with keys were made (there were none). The only network checks were unauthenticated CORS preflight (`OPTIONS`) requests against each provider's endpoint, noted where used. Prices and model IDs are as listed on the providers' pages that day.

**Answer.** Build one typed **scene edit API** in Rust (a new crate, `vxr-ops`) and put everything AI on top of it: the in-app assistant calls it through the model's tool calling, and `vxr mcp` (a stdio MCP server) exposes the same operations to Claude Code and other terminal agents. The model sees the scene as a short text summary plus slices and, for vision models, a rendered picture of the view; it never gets the raw voxels. Every assistant change lands as a staged edit that the user previews (ghost voxels plus counts) and then applies as one undo step, or discards; a terminal agent's edits apply straight away, one undo step each, in plain view. Keys stay on the user's machine: OS keychain on desktop, the tab's memory in the browser by default. All five provider families can be called straight from the browser with no server. First version: Anthropic and any OpenAI-compatible endpoint, the assistant as a docked chat panel with Cmd-K "Ask AI" as a shortcut into the same conversation, the review card, and `vxr mcp` editing `.vox` files on disk. Driving a running desktop app from the terminal comes second. Not built: a text-to-voxel generator.

## Mocks

Throwaway HTML mocks in the Hybrid look, in the git-ignored `.scratch/ai-mocks/` (not committed). Open `.scratch/ai-mocks/index.html` in a browser, or look at the 1x and 2x screenshots in `.scratch/ai-mocks/shots/` (1440x900 at device pixel ratio 1 and 2, taken in headless Chrome). Each in-app mock is laid out at 1280x800 over a real `--shot` screenshot of the editor and scaled to the viewport. The ghost voxels in the review mock are hand-placed outlines, not real voxels.

| Mock | File | What it shows |
|---|---|---|
| Assistant, direction A | `assistant-panel.html` | A docked panel on the right (Cmd-J or the sparkles button): conversation, the tool calls each answer made, model and cost line, composer with "send a picture of the view" and Stop |
| Assistant, direction B | `assistant-inline.html`, `assistant-inline-result.html` | Cmd-K "Ask AI" (Tab asks AI from any row); then a one-line prompt bar at the top and a "Proposed change" card beside the edited area, with "Show steps" opening the full review |
| Review step | `review.html` | The proposed change drawn as ghost voxels (added, removed, repainted), counts, per-step checkboxes, Before/After, Apply (Enter), Discard (Esc), "Ask for changes" |
| Keys, desktop | `keys.html` | Providers, default models, masked keys with Test, "Stored in the macOS Keychain", a change-size limit and a spend reminder |
| Keys, browser | `keys-web.html` | The same, with "This tab only" (default) against "Remember in this browser" and what each risks |
| MCP setup | `mcp.html` | The `claude mcp add` line and the JSON snippet, Files and Live modes, the pairing token, connected clients and the tool list |
| MCP live | `mcp-live.html` | A terminal agent editing the window: a "Claude Code is editing" bar with Stop, and an "Agent edits" card listing each applied call with its own Undo |

## 1. Bring your own key

### Providers and default models

| Provider | Default model | Faster / cheaper | Price per M tokens (in / out) | Wire format | Source |
|---|---|---|---|---|---|
| Anthropic | `claude-opus-5-5` | `claude-sonnet-5-5`; `claude-haiku-5-5` | $4 / $20; Sonnet $2 / $10; Haiku from $0.10 / $0.50 | Messages API | [Models overview](https://platform.claude.com/docs/en/models/overview) |
| OpenAI | `gpt-6.1-sol` | `gpt-6-luna` | $2 / $10; Luna $0.10 / $0.50; flagship `gpt-6-astra` $10 / $50 | Chat Completions, Responses | [Models](https://developers.openai.com/api/docs/models), [Pricing](https://developers.openai.com/api/docs/pricing) |
| Google | `gemini-3.8-flash` | `gemini-3.5-flash-lite` | $0.75 / $3.75 until 2026-12-31, then $1.50 / $7.50; flagship `gemini-4-argon` $2 / $10 until 2026-11-30, then $4 / $20 | Gemini API, plus an OpenAI-compatible endpoint | [Models](https://ai.google.dev/gemini-api/docs/models), [Pricing](https://ai.google.dev/gemini-api/docs/pricing) |
| OpenRouter | whatever the user picks; suggest the Anthropic default | any | the routed provider's price | OpenAI-compatible | [OAuth PKCE](https://openrouter.ai/docs/guides/overview/auth/oauth) |
| Local: Ollama, LM Studio | the user's installed model, listed from the server | - | free; nothing leaves the machine | OpenAI-compatible (`/v1`) | [Ollama FAQ](https://docs.ollama.com/faq), [LM Studio `server start`](https://lmstudio.ai/docs/cli/server-start) |

- **Three client implementations cover all five.** The Anthropic Messages API, OpenAI's Responses API, and one OpenAI-compatible Chat Completions client with a base URL (OpenRouter, Gemini's `/v1beta/openai/` endpoint, Ollama, LM Studio). OpenAI's default `gpt-6.1-sol` refuses function tools on Chat Completions (risk tests 1 and 3, `spikes/ai-edit-quality/RESULTS.md`, `spikes/ai-browser-calls/RESULTS.md`), so OpenAI goes through Responses. A native Gemini client can come later if its OpenAI layer falls short on tool calls or images.
- **Defaults follow the rule "latest, most capable"**: `claude-opus-5-5` is Anthropic's recommended start for most workloads. `claude-fable-5-1` ($10 / $50) is stronger for long reasoning but slower and 2.5x the price; voxel edits are short tool loops, so it is an option in the picker, not the default.
- **Owner's decision after risk test 1 (2026-10-27):** the picker offers `claude-opus-5-5`, `gpt-6.1-sol` and `claude-sonnet-5-5`; the default follows the key: Opus for an Anthropic key, `gpt-6.1-sol` for an OpenAI key, Opus suggested for OpenRouter. Sonnet is listed as the fast option for small scenes. Reason: in round 2, judged blind, Opus and `gpt-6.1-sol` each had 14 of 18 answers accepted, Sonnet 8, all of them on the simple and medium scenes, none on the large one, at about half Opus's time (`spikes/ai-edit-quality/RESULTS.md`).
- **Model IDs change every few months.** Do not hard-code a list. Ship the defaults above, and fill the picker from each provider's models endpoint (Anthropic's Models API reports capabilities per model, [docs](https://platform.claude.com/docs/en/models/overview#using-the-models-api)).
- **Vision is needed** for the rendered view (section 2). All current Claude models take images ([Vision](https://platform.claude.com/docs/en/build-with-claude/vision)); the OpenAI and Gemini defaults above are multimodal. Local models often are not: the assistant must work without the picture.

### Calling providers from the browser (no server)

decisions.md says no server, so the browser build has to call providers directly. CORS preflights from a made-up origin (`https://vxr.example.com`, unauthenticated `OPTIONS`, 2026-10-25):

| Endpoint | Preflight | Notes |
|---|---|---|
| `api.anthropic.com/v1/messages` | allowed (`*`) | The real request must also send `anthropic-dangerous-direct-browser-access: true`, or it fails with a 401 that names the header ([Willison, 2024](https://simonwillison.net/2024/Aug/23/anthropic-dangerous-direct-browser-access)). It exists for bring-your-own-key apps like this one |
| `api.openai.com/v1/chat/completions`, `/v1/responses` | allowed | The official JS SDK refuses browser use unless `dangerouslyAllowBrowser` is set ([JSR docs](https://jsr.io/@openai/openai@6.49.0/doc/~/ClientOptions.dangerouslyAllowBrowser)); vxr would use plain `fetch`. Community threads report the Chat Completions preflight breaking for a while in 2025 ([OpenAI forum](https://community.openai.com/t/chat-completions-api-endpoint-down-blocked-any-web-browser-request/1362527)), so treat it as allowed but not promised |
| `generativelanguage.googleapis.com` (native and `/v1beta/openai/`) | allowed for `content-type`, `x-goog-api-key` and `authorization`; a request listing unknown headers got 403 | Google advises against client-side keys in production ([key docs](https://ai.google.dev/tutorials/setup)); with a user's own key that warning is about the user's own exposure |
| `openrouter.ai/api/v1/chat/completions` | allowed (`*`) | OAuth PKCE gives the user a key without pasting one, and works from a static page: the code exchange has no client secret ([OAuth PKCE](https://openrouter.ai/docs/guides/overview/auth/oauth)) |
| Ollama `http://127.0.0.1:11434` | only `127.0.0.1` and `0.0.0.0` origins by default | The user has to add vxr's origin to `OLLAMA_ORIGINS` ([FAQ](https://docs.ollama.com/faq)) |
| LM Studio `http://localhost:1234/v1` | off by default | `lms server start --cors` or the Enable CORS switch; LM Studio warns this exposes the server ([docs](https://lmstudio.ai/docs/cli/server-start)) |

- A public HTTPS page calling a local server also hits Chrome's **Local Network Access** permission prompt (enforced since Chrome 142, [Chrome blog](https://developer.chrome.com/blog/local-network-access), [OpenReplay summary](https://blog.openreplay.com/chrome-local-network-access-lna-permission/)). The local-model path in the browser is therefore: set `OLLAMA_ORIGINS`, then accept a browser prompt. The keys screen should say so in two lines. Natively there is no such hurdle.
- **No proxy is needed for any provider.** Streaming responses (SSE over `fetch`) still need a check with real keys: risk test 3.

### Storing keys

- **Desktop:** the OS keychain through the `keyring` crate (4.2: macOS and iOS Keychain, Windows Credential Manager, Linux Secret Service and keyutils, [docs.rs](https://docs.rs/keyring/latest/keyring/)). Never in a settings file, never in a `.vox`. The key is read when a request is sent and not kept in UI state.
- **Browser, the risks:** any script running on the editor's origin can read a key the page can use. That means an XSS bug or a compromised dependency on vxr.jrnxf.co, not other sites. A key in `localStorage` also sits on disk in the clear and survives the session. Wrapping it in a non-extractable WebCrypto key in IndexedDB stops someone copying the browser profile from reading it, but not a script on the page, which can still ask the browser to decrypt. Because the page has to send the key itself, no browser storage protects it from code on the page.
- **Browser, where the key lives (settled 2026-10-25).** Three choices, and a key is never stored anywhere the server can read it:
  1. Default: **this tab only**. The key lives in wasm memory and is gone when the tab closes.
  2. Opt-in, off by default: **remember in this browser**, stored encrypted on the device as above, with the warning in plain words (as mocked in `keys-web.html`).
  3. Later: **sync to my account**. The browser encrypts the key with a passphrase only the user knows before uploading it, so the database (D1) holds only an unreadable copy. It comes with Google login, when sharing and embeds are built; the no-server rule holds until then.
  - Considered and rejected: keeping keys in a database the server can read, in plain or encrypted with a server-held secret. One breach would leak every saved key, stolen AI keys are resold, and the operator would become the custodian of everyone's keys.
  - The limit above is unchanged for all three: code running on vxr.jrnxf.co itself can read the key while the page uses it. Items 4 and 5 below limit the damage.
  4. Prefer keys the user can limit: OpenRouter's PKCE key (spend limits per key), or a provider key with a spend cap and a project of its own. The settings screen links to where to set them.
  5. Keep the attack surface small: a strict Content Security Policy on the editor page (`connect-src` only to the provider hosts the user turned on, no third-party scripts), which the static Cloudflare deploy can send as a header.
- Local models need no key.

## 2. How AI edits a scene

### Three approaches

| | (a) Tool calls on a scene edit API | (b) The model writes a script, run in a sandbox | (c) Generate whole models |
|---|---|---|---|
| What the model emits | Typed calls: `box`, `line`, `sphere`, `fill`, `paint`, `select`, `transform`, `palette_set`, `material_set`, queries | A short program (Lua, Rhai, JS in a wasm sandbox) that loops and calls the same primitives | A finished voxel grid or mesh, voxelised on import |
| Editability | High: works on the user's scene, palette slots and selection, so "change the hat" is one more call | High, and the script itself can be kept and rerun as a procedural brush | Low: the quoted complaint about LLM voxel output is that you "can't just change the 'hat'" ([HN](https://news.ycombinator.com/item?id=49590523)) |
| Good at | Targeted edits, recolours, adding parts, layout | Repetition and procedure: a fence of 40 posts, terrain, stairs, a spiral | Brand new organic shapes |
| Cost | Grows with the number of calls; big shapes are one call | One answer, any size | Expensive models or a third-party service; Meshy dropped its text-to-voxel API in 2025 ([changelog](https://docs.meshy.ai/en/api/changelog)) |
| Risk | Low: only typed, bounded operations | Needs a real sandbox (time, memory, no I/O) and an extra runtime in the wasm build | Not editable; not ours |
| Prior art | MagicaVoxel MCP repos (about 40 tools, [repo](https://github.com/Mahinika/magicavoxel-mcp)); MineBench's tool mode with `block`, `box`, `line` "to build larger structures that would exceed JSON token limits" ([repo](https://github.com/Ammaar-Alam/minebench)) | vengi's Lua scripts and brush scripts ([docs](https://vengi-voxel.github.io/vengi/LUAScript/)); Blockbench MCP's `risky_eval` ([repo](https://github.com/jasonjgardner/blockbench-mcp-plugin)) | Meshy, Tripo, VoxelAI Studio (`docs/market-research.md` 1c) |

**Pick (a), and keep (b) as a later step on the same API.** Tool calls give the user exactly the edits a human would make, are easy to preview and bound, and work with every provider that does tool calling, local models included. Shape primitives (`box`, `line`, `sphere`, `fill`) keep big changes to a few calls. A script mode later is the same operations called from a sandboxed language, which is also the "shaders" (procedural brushes) design note in the gap doc: one runtime for both. Whole-model generation stays out, as decisions.md and the market research already say.

### The edit API (`vxr-ops`)

One crate, no GPU, on top of `vxr-core`. Each operation is a serde type with a JSON Schema, so the same definitions become Anthropic tools, OpenAI functions and MCP tools without hand-written copies.

- **Queries:** `scene_summary`, `palette`, `get_voxels(region)` (capped at 4,096 cells), `slice(axis, index, region)`, `find(color | name)`, `render_view(camera?, size?)`.
- **Edits:** `place`, `erase`, `box`, `line`, `sphere` (and circle), `fill(region | selection)`, `paint(region | selection | color, slot)`, `select(region | color, mode)`, `transform(move | rotate | flip | loop, offset | axis)` (the selection, or the whole model when nothing is selected; refused, moving nothing, when a voxel would leave the bounds), `copy`, `cut`, `paste(at)` (the agent's own clipboard), `delete`, `hollow`, `fill_inside(slot)`, `resize(size)` (grow the bounds before building beyond them), `crop`, `palette_set(slot, rgba)`, `material_set(slot, type, sliders)`, and in a world of several objects `move_object(node, offset)`, `duplicate(node, linked)`, `group(nodes)`, `ungroup(node)`, `hide(node, hidden)` and `set_layer(node, layer | new_layer)` (nodes as the summary's world list numbers them; each returns the nodes it changed, as a world change has no cells).
- **Control:** `undo` (of the AI's own staged steps only), `done(summary)`.
- Every edit returns a short result: cells changed, new bounds, and a warning when the operation clipped or did nothing. The model needs this feedback to correct itself, and it costs a few tokens.
- `vxr-core` already has the right shape: `Scene::begin` opens an edit, `commit` turns it into one undo step, `revert_voxels` drops voxel changes, and selection changes are recorded in the same step. An AI turn is one open edit.

### How the model sees the scene

The island is about 145k voxels; a 256^3 model can hold 16.7M. Sending raw voxels is not possible, and not useful.

| View | Size | When |
|---|---|---|
| `scene_summary`: bounds, voxel count, per-slot counts with hex colours and material types, the selection's bounds and count, the camera, object and layer names | 300 to 1,500 tokens | Always, at the start of each turn |
| A height map or top-down run-length text of the model (coarse, 2 to 4 cells per character) | 500 to 2,000 tokens | When placing things "on top of" or "next to" |
| `slice(axis, index)` as text rows, one character per palette slot | a 64x64 slice is about 1,500 tokens | When the model asks |
| `get_voxels(region)` | capped | Small exact areas |
| `render_view`: a PNG of the current view, or an axis view, from the existing renderer, with axis labels and a grid drawn in | 1000x625 is 36 x 23 = 828 visual tokens on Claude ([Vision](https://platform.claude.com/docs/en/build-with-claude/vision)) | Vision models, at the start and after a big change, so the model can check its own work |

The user's selection and the voxel under the pointer are the cheapest context of all: "make this red" with a selection needs no picture.

### Keeping a change reviewable

1. **Stage, do not apply.** The AI's calls run inside one open edit on the live scene. The editor draws it as a ghost: added voxels tinted, removed ones outlined, repainted ones with an accent edge (the selection highlight already does this kind of tint in the march shader). The render corner traces the same map, so it shows the after state except staged removals, which stay in the map until Apply (`spikes/ai-staged-ghost/RESULTS.md`).
2. **Review card:** counts (`+1,248 added · 96 removed · 412 repainted · slot 3 changed`), the list of steps with checkboxes to drop one, Before/After, and a field to ask for changes, which continues the same staged edit.
3. **Apply** commits it as one undo step named after the prompt ("AI: make the roof red"). **Discard** or Escape reverts it. **Stop** cancels the request mid-stream and keeps or drops what was staged, the user's choice.
4. **Limits per request:** at most 40 tool calls and 200k changed cells by default; over 10k cells the card asks before staging more. Palette and material changes are always listed by slot.

## 3. MCP: `vxr mcp`

### What it drives

Both, through one binary:

- **Files mode (first).** `vxr mcp --root <dir>` edits `.vox` files on disk headlessly, like the MagicaVoxel MCP repos ([example](https://github.com/Mahinika/magicavoxel-mcp)) and Blockbench's headless stdio server with its `--root` limit ([repo](https://github.com/jasonjgardner/blockbench-mcp-plugin)). vxr's edge: saves are lossless through `vxr_vox::Document`, so materials, layers, cameras and names survive, which is where the MagicaVoxel MCP repos fall short ("complex features such as materials and animations may need manual editing", same README). Writes go to a temporary file and are renamed into place; the previous version is kept as `<name>.vox.bak` once per session.
- **Live mode (second).** The running desktop app listens on a local socket when the user turns on "Allow terminal agents to edit this window". `vxr mcp --live` connects to it and forwards the same operations. vengi works this way: its MCP process "joins VoxEdit's network as a normal client" with a host, port and password ([MCP.md](https://github.com/vengi-voxel/vengi/blob/master/docs/voxedit/MCP.md)).
  - Use a **Unix domain socket** (a named pipe on Windows) in the user's runtime directory, not a TCP port. A web page cannot open one, so DNS rebinding, the attack the MCP spec warns about for local HTTP servers ([Streamable HTTP, Security](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http.md)), does not apply. Blockbench's plugin listens on HTTP with "no authentication" and a JavaScript `risky_eval` tool; vxr should not repeat that.
  - Pairing: the app writes a random token to a file only the user can read (mode 0600) next to the socket, and `vxr mcp --live` reads it. Regenerate turns old clients away.
  - Each MCP edit call is applied straight away as one undo step labelled with the client's name ("Claude Code: paint 412 voxels"); a chip in the hint bar shows a connected agent with a Stop button. MCP clients already ask the user before each tool call, so a second review card per call would be noise; a "review every agent change" switch covers users who want it. An "Agent edits" card lists the calls with an Undo on each, and the agent can call `undo` itself when the user asks it to take something back (`mcp-live.html`).
- **Whose model.** The model runs in the user's agent, so its use counts against that agent's plan: from Claude Code, the user's Claude subscription. `vxr mcp` makes no model calls and needs no key. The in-app assistant is different: a third-party app cannot sign in with a Claude subscription, so it needs an API key.
- **The browser build** cannot host a socket or a stdio server. The only route is the page opening a WebSocket to a local `vxr` process, which needs a pairing code, an Origin check and Chrome's Local Network Access prompt. It was deferred until a local binary was one command away; with `brew install jrnxf/tap/vxr` it is, and `vxr bridge` is that process (built 2026-10-29, `docs/decisions.md` "`vxr bridge`"): it hosts the socket `vxr mcp --live` already connects to, and the page pairs with it over a loopback WebSocket with a code the user carries from the terminal.

### Tools and resources

- **Tools:** the `vxr-ops` operations from section 2, generated from the same schemas, plus `open(path)`, `save(path?)` and `list_files` in files mode. One tool per operation, not one generic `exec`, so clients can show and approve each call.
- **Resources:** `vxr://scene/summary`, `vxr://scene/palette`, `vxr://scene/view.png` (a render of the current view), and in files mode one resource per `.vox` under the root.
- **Transport:** stdio, newline-delimited JSON-RPC, current spec revision 2026-07-28 ([Transports](https://modelcontextprotocol.io/specification/latest/basic/transports)). The live socket reuses stdio framing, as the spec recommends for custom byte-stream transports. Setup is one line: `claude mcp add vxr -- vxr mcp`, or the JSON `{"mcpServers": {"vxr": {"command": "vxr", "args": ["mcp"]}}}`.

### One implementation

```
          in-app assistant ──┐                 ┌── vxr mcp (files) ── vxr_vox::Document on disk
 (Anthropic / OpenAI-compat  │                 │
  client, review card)       ├─► vxr-ops ──────┤
                             │  (typed ops,    └── vxr mcp --live ── socket ── desktop app's Scene
          later: scripts ────┘   JSON Schema)
```

The assistant, the MCP server and a later script runtime are three front ends; the operations, their limits and their tests live once in `vxr-ops`.

## 4. Prior art, and what it gets wrong

| Project | What it is | Lesson |
|---|---|---|
| [magicavoxel-mcp](https://github.com/Mahinika/magicavoxel-mcp) (29 stars; about eight similar repos, `docs/market-research.md`) | Python MCP server that writes `.vox` files directly, about 40 tools (shapes, patterns, palette, transforms) | Shape tools are right. Its `.vox` support is basic (materials and animation lost), 256^3 cap, default palette, Windows paths. Nobody sees the change before it lands |
| [Blockbench MCP plugin](https://github.com/jasonjgardner/blockbench-mcp-plugin) (570 stars) | In-app HTTP server on port 3000 plus a headless stdio server for `.bbmodel` files | Good: both modes, loopback-only with Origin checks, a root directory limit. Bad: no authentication and a JavaScript eval tool |
| [vengi MCP](https://github.com/vengi-voxel/vengi/blob/master/docs/voxedit/MCP.md) and [Lua scripting](https://vengi-voxel.github.io/vengi/LUAScript/) | MCP client of a running VoxEdit with password; a screenshot tool rendered without a GL context; Lua generator, brush and selection scripts | Rendering for the model is worth having; scripting and brushes can share one runtime |
| [MineBench](https://github.com/Ammaar-Alam/minebench) (341 stars), [VoxelBench](https://benchlm.ai/benchmarks/voxelbench) | LLM build benchmarks: raw JSON coordinates, or a `voxel.exec` tool with `block`, `box`, `line` | Primitives beat coordinate lists. The output is throwaway |
| "Voxel pagoda garden" prompt ([X](https://x.com/grok/status/2062774888054563080)) | Informal LLM benchmark, usually three.js code | The complaint is editability: you "can't just change the 'hat' of the Pelican" ([HN](https://news.ycombinator.com/item?id=49590523)) |
| Text-to-voxel: [Meshy](https://www.meshy.ai/features), [Tripo](https://www.tripo3d.ai/), [VoxelAI Studio](https://voxelai.studio), [VoxiGen](https://discourse.threejs.org/t/voxigen-ai-powered-browser-voxel-editor-generate-edit-3d-models-from-text-images/89926) | Generators with a voxel style or small editors around one | A mesh in voxel style is not an editable palette grid; Meshy dropped its voxel API |

What they get wrong, together: the model's output is a final object, not edits to the user's scene; nothing can be reviewed before it lands; and saves lose what the tool does not understand. vxr's answer is edits on the real scene, a review step, and lossless `.vox`.

## 5. Safety and cost

### Prompt injection

- A `.vox` from a stranger carries text the model will read: object and layer names (`_name`), `NOTE` chunk colour names, file names. That is indirect prompt injection: "an LLM accepts input from external sources, such as websites or files" ([OWASP LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/)).
- **The in-app assistant has no way to leak data**: its only tools are scene edits and queries, and its only outbound connection is the provider the user chose. Of the "lethal trifecta" (private data, untrusted content, a way to communicate out, [Willison](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/)) it lacks the third, so the worst a hostile file can do is steer edits, which the review card shows before anything lands.
- **The MCP server is different**: it returns file text to an agent that may have a shell and the web. Mitigations, all in `vxr-ops`: cap names at 64 characters and strip control characters, return them as quoted data in a field marked as file content, never as instructions, and keep `vxr mcp` free of any network or shell tool and limited to its `--root` (OWASP's least privilege and human approval).
- Limits per request as in section 2; a change over the limit needs the user's click.

### Cost per typical request (estimate)

A typical request: "make the tower roof red and add a flag on top", 4 model turns. System prompt and about 20 tool schemas, about 2,700 tokens (plus 286 tokens of tool-use system prompt on current Claude models, [Tool use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview)); scene summary 1,200; one 1000x625 view 830; each turn adds about 1,000 tokens of calls and results. That is about 28k input tokens over the 4 turns (each resends the conversation) and about 2,000 output tokens. Prompt caching cuts the resent part to 5% of the input price on Opus 5.5 and Sonnet 5.5 ([Models overview](https://platform.claude.com/docs/en/models/overview)).

| Model | Without caching | With caching (approx.) |
|---|---|---|
| `claude-opus-5-5` | $0.15 | $0.08 |
| `claude-sonnet-5-5` | $0.08 | $0.04 |
| `gpt-6.1-sol` | $0.08 | $0.04 |
| `gemini-3.8-flash` | $0.03 | not estimated |
| Local (Ollama) | $0 | $0 |

A big build ("a pagoda garden on the empty scene") is perhaps 10 to 20 turns and 5 to 10 times that: under a dollar on the default model. Risk test 1 replaces these estimates with measurements.

### Privacy

What leaves the machine, only to the provider the user picked: the prompt, the scene summary (including object and layer names), the slices or voxels the model asks for, and the rendered view when vision is on. Never: the file path outside the summary, other files, the key to anyone but its provider. The composer's "send a picture of the view" toggle and a one-line notice on first use make this visible. Local models keep everything on the machine.

## 6. Recommendation

### Build

1. `vxr-ops`: the typed operation set with JSON Schemas, its limits, the scene summary, slices and `render_view`, with unit tests and a `--shot` section that drives a scripted tool-call sequence through the same staging and review path.
2. The **review step** in the editor: staged edit, ghost overlay, review card, one undo step, Stop.
3. **The assistant as both directions, sharing one conversation:** the docked chat panel (direction A, Cmd-J) is its home, with history and the tool calls; Cmd-K "Ask AI" (direction B) is the quick way in, and its result card can open in the panel. Small changes review in the card; bigger ones open the step list (`review.html`). The owner picked this on 2026-10-25.
4. **Providers:** Anthropic Messages, OpenAI Responses and one OpenAI-compatible client (Gemini's OpenAI endpoint, OpenRouter with PKCE, Ollama, LM Studio). Keys in the keychain natively and per tab in the browser.
5. `vxr mcp` **files mode**, then **live mode** over a local socket.

### Small enough to ship first

Assistant in the docked panel and from Cmd-K, with review, Anthropic, OpenAI and OpenAI-compatible, 12 operations (`scene_summary`, `render_view`, `place`, `erase`, `box`, `line`, `sphere`, `fill`, `paint`, `select`, `palette_set`, `done`), natively and in the browser. `vxr mcp` files mode with the same 12 plus `undo`, `open` and `save`. No scripts, no live mode.

### Risk tests first (like Phase 2)

Listed with thresholds in `docs/plan.md`, phase "AI". In short: can models actually edit well through these tools (an eval of 20 prompts), does the picture help, do direct browser calls stream, does a hostile file get ignored, and does `vxr mcp` round-trip files losslessly from Claude Code.

### Open questions for the owner

1. ~~Is AI still out of v1 (decisions.md says so), with this as the first phase after it, or does the assistant ship in v1?~~ Answered 2026-10-25: the assistant and `vxr mcp` both ship in v1.
2. ~~Direction A (panel) or B (Cmd-K and a card) for the assistant?~~ Answered 2026-10-25: both, sharing one conversation, with the panel as home.
3. ~~Should vxr ever offer a hosted key or proxy (a paid tier from the market research), or stay strictly bring-your-own-key?~~ Answered 2026-10-25: do not plan the hosted tier yet.
4. ~~Is "remember in this browser" worth offering at all, given what it cannot protect against?~~ Answered 2026-10-25: three choices for where a browser key lives, and never anywhere the server can read it: this tab only (default), remember in this browser (encrypted on the device, off by default, with the warning), and later sync to my account (encrypted in the browser with the user's own passphrase before upload, so D1 holds only an unreadable copy; comes with Google login). Server-readable storage was rejected.
5. ~~Live MCP: apply each agent call straight away with undo, or review every call?~~ Answered 2026-10-25: apply straight away, one undo step each, visible on screen, and the agent can undo on request.
6. ~~Script mode (b) on the same API: Lua (vengi-compatible scripts) or Rhai/JS in a wasm sandbox?~~ Answered 2026-10-25: pass on scripting for now; the script language is decided when script mode is planned.
