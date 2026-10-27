# Test 3: direct browser calls

Phase AI risk test 3 (`docs/plan.md`). Run 2026-10-27 on the M4 Pro in headless Chrome 156, from a throwaway `*.workers.dev` origin (real HTTPS, a Worker of its own, never routed to vxr.jrnxf.co), with the owner's Anthropic and OpenAI keys; OpenRouter from the owner's own browser after signing in.

**Result: pass for Anthropic, OpenAI and OpenRouter; Gemini and Ollama not run.** Anthropic and OpenAI each streamed a `place` tool call straight from the page, three runs each, and OpenRouter once through a key from its PKCE sign-in, all with no proxy and under a strict CSP that blocked a host it does not list. OpenAI's default model, `gpt-6.1-sol`, refuses function tools on Chat Completions; it needs the Responses API, so "one OpenAI-compatible client covers OpenAI" does not hold for the default. Gemini's OpenAI endpoint answered the page across CORS, but Google rejected the key in the key file on every endpoint, so no Gemini tool call was made. Ollama is not installed on this Mac: not run. A stand-in on Ollama's port showed the browser side of the local path: Chrome's Local Network Access gate refuses the call until the user allows it, and with it allowed the stream passes CSP and CORS.

## What was built

A static page (`www/`): `index.html`, `app.js` (no dependencies), `style.css`, and `_headers`, which Cloudflare's static assets send on every response:

```
Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self';
  connect-src https://api.anthropic.com https://api.openai.com https://generativelanguage.googleapis.com
  https://openrouter.ai http://127.0.0.1:11434 http://localhost:11434; base-uri 'none'; form-action 'none';
  frame-ancestors 'none'; require-trusted-types-for 'script'
Referrer-Policy: no-referrer
```

No inline script or style, no third-party code, Trusted Types required (the page writes only `textContent`). `wrangler.jsonc` deploys it as its own Worker on workers.dev.

- **Three clients over plain `fetch`, each streaming server-sent events:** Anthropic Messages (`x-api-key`, `anthropic-version`, `anthropic-dangerous-direct-browser-access: true`); OpenAI-compatible Chat Completions with a base URL (OpenAI, Gemini's `/v1beta/openai`, OpenRouter, Ollama), with `stream_options.include_usage`; and OpenAI's Responses API (`store: false`), added when Chat Completions refused tools on the default model.
- **One tool,** `place(x, y, z, color)`, and one prompt: "Put a red voxel at x 3, y 1, z -2." A run passes when the stream carries a `place` call with x 3, y 1, z -2. Each run records time to headers, first and last argument delta, body reads, SSE events, argument deltas, stop reason, usage and any CSP violation.
- **The key** is typed into a password field, read and cleared when the run starts, and kept only in the module's memory. Provider error text is scrubbed of anything key-shaped before it reaches the page.
- **CSP control:** a `fetch('https://example.com/')` that must fail and raise a `securitypolicyviolation` event.
- **OpenRouter PKCE:** a SHA-256 challenge from a random verifier kept in `sessionStorage`, a redirect to `openrouter.ai/auth` with the page as callback, and on return the code exchanged at `/api/v1/auth/keys` for a key that stays in memory; the page then runs the OpenRouter call (`anthropic/claude-opus-5.5`, OpenRouter's ID for the Anthropic default). `?signin` starts the flow on load.
- `local-standin.mjs`: a stand-in on 127.0.0.1:11434 that answers CORS only for origins in `OLLAMA_ORIGINS` (as Ollama does) and streams a canned `place` call in four chunks. Not a model.
- `run <url> <out-dir> [runs [provider[=model]...]]`: drives the page with `chrome-devtools-axi` (fresh page per run, the key typed into the field, never printed), writes `results.ndjson` and a screenshot, and fails if a key appears in its output.

## Numbers

Three runs each, all from the page on the workers.dev origin. Times in ms from the request.

| Provider, API | Model | Pass | Headers | First arg delta | Last arg delta | Body reads | Arg deltas | Tokens in / out |
|---|---|---|---|---|---|---|---|---|
| Anthropic Messages | `claude-opus-5-5` | 3 of 3 | 944-1416 | 945-1417 | 1598-2058 | 4 | 9 | 457 / 113 |
| OpenAI Chat Completions | `gpt-6.1-sol` | 0 of 3 | 189-1170 | - | - | - | - | HTTP 400 |
| OpenAI Responses | `gpt-6.1-sol` | 3 of 3 | 419-667 | 1767-2246 | 2117-2480 | 17-23 | 21 | 88 / 33 |
| OpenAI Chat Completions, `reasoning_effort: "none"` | `gpt-6-luna` | 3 of 3 | 778-1261 | 797-1275 | 939-1539 | 12-16 | 21 | 170 / 32 |
| OpenAI Responses | `gpt-6-luna` | 3 of 3 | 466-510 | 736-933 | 890-1099 | 21-29 | 21 | 88 / 33 |
| Gemini OpenAI endpoint | `gemini-3.8-flash` | not run | 178 | - | - | - | - | HTTP 401, key rejected |
| OpenRouter (PKCE key), owner's browser | `anthropic/claude-opus-5.5` | 1 of 1 | 995 | 2725 | - | - | 5 | 457 / 113 |
| Stand-in on 127.0.0.1:11434, LNA allowed | (canned) | 1 of 1 | 4 | 36 | 129 | 5 | 4 | - |

Every passing run in headless Chrome streamed: the arguments arrived as several deltas over 140 to 660 ms, across several body reads, and the page parsed them as they came. The OpenRouter run reported only the first delta (2725 ms) and the total (2742 ms), so its 5 deltas came within 17 ms. Anthropic sends its 9 argument deltas in about 4 network reads. No run raised a CSP violation; the CSP control was refused every time (`connect-src`, `https://example.com/`). The key field was empty after every run, and no key appears in the results or screenshots.

Spend: 4 Anthropic calls on `claude-opus-5-5` (about $0.016), 1 OpenRouter call on the owner's OpenRouter account ($0.004088 as OpenRouter reported), 4 Responses calls on `gpt-6.1-sol` and 8 calls on `gpt-6-luna` (under $0.003 together); failed calls are not billed. About $0.025 in all, against a $15 cap.

## Per provider

- **Anthropic:** works as the desk research said. The direct-browser-access header is the only extra; without a key the page gets a readable 401 ("x-api-key header is required"), so CORS is open.
- **OpenAI:** CORS is open on both `/v1/chat/completions` and `/v1/responses`. On Chat Completions, `gpt-6.1-sol` and `gpt-6-luna` answer "Function tools with reasoning_effort are not supported for <model> in /v1/chat/completions. To use function tools, use /v1/responses or set reasoning_effort to 'none'". `gpt-6-luna` accepts `"none"` and then streams the call; `gpt-6.1-sol` refuses `"none"` (only low, medium, high and xhigh). So the default model needs the Responses API, where both models stream the call with reasoning at its default.
- **Gemini:** `generativelanguage.googleapis.com/v1beta/openai/chat/completions` answered the page with a readable 401, so CORS and the CSP allow it. Google rejected the key in the key file: "API key not valid" (`ACCESS_TOKEN_TYPE_UNSUPPORTED`) as a Bearer token on the OpenAI endpoint, and the same as `x-goog-api-key` or `?key=` on the native `generateContent` endpoint and on Vertex AI. It needs a current Google AI Studio key; the page and the run script are ready for it.
- **OpenRouter:** the models list and `/api/v1/auth/keys` allow any origin; a made-up code sent from the page reached OpenRouter's validation (a 400 naming the field). The owner opened `<url>?signin`, signed in, and came back to the page, which exchanged the code for a key (200) and streamed the call with it: `place` with the right cell in 5 deltas, stop reason `tool_calls`, no CSP violation, usage with OpenRouter's own `cost`. The key never left that tab. One run only, as it needs a person at the sign-in.
- **Ollama:** not run; it is not installed on this Mac. What the stand-in showed about the browser side:
  - **Local Network Access** (Chrome 142 and later): by default headless Chrome reports the `local-network-access` permission as `denied` and the call fails before reaching the port: "Permission was denied for this request to access the `loopback` address space". A user sees a prompt in its place. With the gate off (`--disable-features=LocalNetworkAccessChecks`, standing in for the user's Allow), the page's preflight and request reached the stand-in and the tool call streamed under the same CSP.
  - **`OLLAMA_ORIGINS`:** without the page's origin in it, the stand-in refused the preflight, as Ollama does, and the call failed.
  - Not covered: the real prompt's wording and whether Allow sticks across visits, and a real Ollama model's tool calls.

## Against the bar

- Every provider streams a tool call with no proxy: Anthropic yes, OpenAI yes (through Responses for the default model), OpenRouter yes (PKCE key, one run), Gemini not run (the key was rejected; the endpoint's CORS is open), Ollama not run (not installed; the stand-in showed the Local Network Access gate).
- A strict CSP still allows it: yes, for every call made. `connect-src` with the five hosts (plus 127.0.0.1 and localhost for local models) is enough, and the same CSP blocked an unlisted host.

## What it changes for the build

- **OpenAI needs the Responses API.** The plan's two clients (Anthropic Messages and one OpenAI-compatible Chat Completions client) do not cover OpenAI's default model: Chat Completions refuses tools on it at any reasoning level it accepts. Use Responses for OpenAI and keep Chat Completions for Gemini, OpenRouter, Ollama and LM Studio. The Responses client is about 40 lines here; its events (`response.output_item.added`, `response.function_call_arguments.delta`, `response.completed`) are as simple as Chat Completions'.
- **The CSP holds as planned.** A `_headers` file on the static Cloudflare deploy carries it; `connect-src` is the provider list, and Trusted Types cost nothing for a page that never writes HTML. The editor's page (wasm, workers, fonts) will need `script-src 'wasm-unsafe-eval'` and whatever else the wasm build loads; not tried here.
- **The local path in the browser is two steps for the user,** as the research said: add vxr's origin to `OLLAMA_ORIGINS`, then accept Chrome's Local Network Access prompt. The keys screen can check the permission with `navigator.permissions.query({ name: 'local-network-access' })` and say which step is missing.
- **Model IDs differ per route:** OpenRouter names the Anthropic default `anthropic/claude-opus-5.5`, not `claude-opus-5-5`. Filling the picker from each provider's models endpoint, as planned, avoids hard-coding either.

## Rerun

```
./dev wrangler deploy --config spikes/ai-browser-calls/wrangler.jsonc   # prints the workers.dev URL
env $(grep -E '^(ANTHROPIC|OPENAI|GEMINI)_API_KEY=' <key file> | xargs) \
  spikes/ai-browser-calls/run <url> <out-dir> 3 anthropic openai openai-responses openai-none=gpt-6-luna gemini
OLLAMA_ORIGINS=<url without the slash> node spikes/ai-browser-calls/local-standin.mjs &   # or a real Ollama
CHROME_DEVTOOLS_AXI_CHROME_ARGS=--disable-features=LocalNetworkAccessChecks spikes/ai-browser-calls/run <url> <out-dir> 1 ollama
./dev wrangler delete --config spikes/ai-browser-calls/wrangler.jsonc    # when done (done after this run)
```

OpenRouter: open `<url>?signin` in a normal browser and sign in; the page shows the result. Results and screenshots go to `<out-dir>`, outside git.
