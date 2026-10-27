// Phase AI risk test 3: one streamed tool call per provider, straight from the browser.
// Anthropic Messages and OpenAI-compatible Chat Completions cover every provider; OpenAI's
// Responses API is here too, since its default model needs it for tools with reasoning on.
// The key is read from the field, the field is cleared, and the key lives only in this module.

const PROVIDERS = {
  anthropic: { kind: 'anthropic', base: 'https://api.anthropic.com/v1', model: 'claude-opus-5-5' },
  openai: { kind: 'openai', base: 'https://api.openai.com/v1', model: 'gpt-6.1-sol' },
  // gpt-6.1-sol refuses function tools on Chat Completions unless reasoning is off.
  'openai-none': { kind: 'openai', base: 'https://api.openai.com/v1', model: 'gpt-6.1-sol', extra: { reasoning_effort: 'none' } },
  'openai-responses': { kind: 'responses', base: 'https://api.openai.com/v1', model: 'gpt-6.1-sol' },
  gemini: { kind: 'openai', base: 'https://generativelanguage.googleapis.com/v1beta/openai', model: 'gemini-3.8-flash' },
  openrouter: { kind: 'openai', base: 'https://openrouter.ai/api/v1', model: 'anthropic/claude-opus-5.5' },
  ollama: { kind: 'openai', base: 'http://127.0.0.1:11434/v1', model: 'qwen3:0.6b' },
};

const TOOL = {
  name: 'place',
  description: 'Place one voxel at a cell of the scene. y is up.',
  schema: {
    type: 'object',
    properties: {
      x: { type: 'integer' },
      y: { type: 'integer' },
      z: { type: 'integer' },
      color: { type: 'string', description: 'Hex colour such as #ff0000' },
    },
    required: ['x', 'y', 'z', 'color'],
  },
};
const PROMPT = 'Put a red voxel at x 3, y 1, z -2.';
const WANT = { x: 3, y: 1, z: -2 };

const $ = (id) => document.getElementById(id);
const violations = [];
window.__results = [];
let openrouterKey = '';

document.addEventListener('securitypolicyviolation', (e) => {
  violations.push({ directive: e.effectiveDirective, blocked: e.blockedURI });
});

function setStatus(text) { $('status').textContent = text; }
function log(text) { $('log').textContent += text + '\n'; }

// Provider error bodies can quote part of a key; never let one reach the page or the result.
function scrub(text, key) {
  let s = String(text).slice(0, 400);
  if (key) s = s.split(key).join('<key>').split(key.slice(0, 12)).join('<key>');
  return s.replace(/\b(sk-[A-Za-z0-9_*-]{6,}|AIza[A-Za-z0-9_-]{10,})/g, '<key>');
}

function pick(provider) {
  const p = PROVIDERS[provider];
  $('model').value = p.model;
  $('base').value = p.base;
}

// Server-sent events from a fetch body, one callback per event, timing every read.
async function readSse(res, t0, onEvent, stats) {
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    stats.reads++;
    stats.lastReadMs = performance.now() - t0;
    buf += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n');
    let i;
    while ((i = buf.indexOf('\n\n')) >= 0) {
      const block = buf.slice(0, i);
      buf = buf.slice(i + 2);
      let event = 'message';
      const data = [];
      for (const line of block.split('\n')) {
        if (line.startsWith('event:')) event = line.slice(6).trim();
        else if (line.startsWith('data:')) data.push(line.slice(5).trimStart());
      }
      if (!data.length) continue;
      stats.events++;
      const d = data.join('\n');
      if (d === '[DONE]') continue;
      onEvent(event, JSON.parse(d));
    }
  }
}

async function callAnthropic(base, model, key, r, t0) {
  const res = await fetch(`${base}/messages`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-api-key': key,
      'anthropic-version': '2023-06-01',
      'anthropic-dangerous-direct-browser-access': 'true',
    },
    body: JSON.stringify({
      model,
      max_tokens: 1024,
      stream: true,
      tools: [{ name: TOOL.name, description: TOOL.description, input_schema: TOOL.schema }],
      messages: [{ role: 'user', content: PROMPT }],
    }),
  });
  r.httpStatus = res.status;
  r.ttfbMs = performance.now() - t0;
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${scrub(await res.text(), key)}`);
  let json = '';
  await readSse(res, t0, (event, d) => {
    if (event === 'message_start') r.usage.input = d.message.usage?.input_tokens;
    else if (event === 'content_block_start' && d.content_block.type === 'tool_use') r.toolName = d.content_block.name;
    else if (event === 'content_block_delta' && d.delta.type === 'input_json_delta') {
      json += d.delta.partial_json;
      r.toolDeltas++;
      r.firstToolDeltaMs ??= performance.now() - t0;
      r.lastToolDeltaMs = performance.now() - t0;
    } else if (event === 'message_delta') {
      r.stopReason = d.delta.stop_reason;
      r.usage.output = d.usage?.output_tokens;
    } else if (event === 'error') throw new Error(scrub(JSON.stringify(d.error), key));
  }, r);
  return json;
}

async function callOpenAi(provider, base, model, key, r, t0) {
  const extra = PROVIDERS[provider].extra ?? {};
  const headers = { 'content-type': 'application/json' };
  if (key) headers.authorization = `Bearer ${key}`;
  if (provider === 'openrouter') headers['x-title'] = 'vxr';
  const res = await fetch(`${base}/chat/completions`, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      model,
      stream: true,
      stream_options: { include_usage: true },
      tools: [{ type: 'function', function: { name: TOOL.name, description: TOOL.description, parameters: TOOL.schema } }],
      messages: [{ role: 'user', content: PROMPT }],
      ...extra,
    }),
  });
  r.httpStatus = res.status;
  r.ttfbMs = performance.now() - t0;
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${scrub(await res.text(), key)}`);
  let json = '';
  await readSse(res, t0, (_event, d) => {
    if (d.error) throw new Error(scrub(JSON.stringify(d.error), key));
    if (d.usage) r.usage = { input: d.usage.prompt_tokens, output: d.usage.completion_tokens, cost: d.usage.cost };
    const c = d.choices?.[0];
    if (!c) return;
    for (const tc of c.delta?.tool_calls ?? []) {
      if (tc.function?.name) r.toolName = tc.function.name;
      if (tc.function?.arguments) {
        json += tc.function.arguments;
        r.toolDeltas++;
        r.firstToolDeltaMs ??= performance.now() - t0;
        r.lastToolDeltaMs = performance.now() - t0;
      }
    }
    if (c.finish_reason) r.stopReason = c.finish_reason;
  }, r);
  return json;
}

async function callResponses(base, model, key, r, t0) {
  const res = await fetch(`${base}/responses`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${key}` },
    body: JSON.stringify({
      model,
      stream: true,
      store: false,
      tools: [{ type: 'function', name: TOOL.name, description: TOOL.description, parameters: TOOL.schema }],
      input: PROMPT,
    }),
  });
  r.httpStatus = res.status;
  r.ttfbMs = performance.now() - t0;
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${scrub(await res.text(), key)}`);
  let json = '';
  await readSse(res, t0, (event, d) => {
    const type = d.type ?? event;
    if (type === 'response.output_item.added' && d.item.type === 'function_call') r.toolName = d.item.name;
    else if (type === 'response.function_call_arguments.delta') {
      json += d.delta;
      r.toolDeltas++;
      r.firstToolDeltaMs ??= performance.now() - t0;
      r.lastToolDeltaMs = performance.now() - t0;
    } else if (type === 'response.completed') {
      r.stopReason = d.response.status;
      r.usage = { input: d.response.usage?.input_tokens, output: d.response.usage?.output_tokens, reasoning: d.response.usage?.output_tokens_details?.reasoning_tokens };
    } else if (type === 'error' || type === 'response.failed') throw new Error(scrub(JSON.stringify(d), key));
  }, r);
  return json;
}

async function run(provider, model, base, key) {
  const r = {
    provider, model, base, origin: location.origin, ok: false, httpStatus: null,
    ttfbMs: null, firstToolDeltaMs: null, lastToolDeltaMs: null, totalMs: null,
    reads: 0, events: 0, toolDeltas: 0, toolName: null, args: null, stopReason: null,
    usage: {}, error: null, violations: [],
  };
  const seen = violations.length;
  const t0 = performance.now();
  setStatus(`${provider}: running`);
  try {
    const kind = PROVIDERS[provider].kind;
    const json = kind === 'anthropic' ? await callAnthropic(base, model, key, r, t0)
      : kind === 'responses' ? await callResponses(base, model, key, r, t0)
      : await callOpenAi(provider, base, model, key, r, t0);
    r.args = JSON.parse(json || 'null');
    r.ok = r.toolName === TOOL.name && !!r.args
      && r.args.x === WANT.x && r.args.y === WANT.y && r.args.z === WANT.z;
    if (!r.ok) r.error = 'no matching place call';
  } catch (e) {
    r.error = scrub(`${e.name}: ${e.message}`, key);
  }
  r.totalMs = performance.now() - t0;
  r.violations = violations.slice(seen);
  for (const k of ['ttfbMs', 'firstToolDeltaMs', 'lastToolDeltaMs', 'totalMs', 'lastReadMs']) {
    if (r[k] != null) r[k] = Math.round(r[k]);
  }
  window.__results.push(r);
  setStatus(`${provider}: ${r.ok ? 'PASS' : 'FAIL'}${r.ok ? ` - streamed ${r.toolName}(${JSON.stringify(r.args)}) in ${r.toolDeltas} deltas` : ` - ${r.error}`}`);
  log(JSON.stringify(r, null, 2));
  return r;
}

// A host outside connect-src must be refused by the page's own CSP.
async function cspControl() {
  const seen = violations.length;
  let blocked = false, error = null;
  try { await fetch('https://example.com/'); } catch (e) { blocked = true; error = `${e.name}: ${e.message}`; }
  await new Promise((r) => setTimeout(r, 50));
  const r = { provider: 'csp-control', ok: blocked && violations.length > seen, blocked, error, violations: violations.slice(seen) };
  window.__results.push(r);
  setStatus(`CSP control: ${r.ok ? 'PASS - example.com refused' : 'FAIL'}`);
  log(JSON.stringify(r, null, 2));
  return r;
}

// OpenRouter OAuth PKCE: no client secret, so a static page can do the whole exchange.
const b64url = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

async function signIn() {
  const verifier = b64url(crypto.getRandomValues(new Uint8Array(32)));
  const challenge = b64url(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier)));
  sessionStorage.setItem('or_verifier', verifier);
  const callback = location.origin + location.pathname;
  location.href = `https://openrouter.ai/auth?callback_url=${encodeURIComponent(callback)}&code_challenge=${challenge}&code_challenge_method=S256`;
}

async function exchange(code) {
  const verifier = sessionStorage.getItem('or_verifier');
  sessionStorage.removeItem('or_verifier');
  history.replaceState(null, '', location.pathname);
  const r = { provider: 'openrouter-pkce', ok: false, httpStatus: null, error: null };
  try {
    const res = await fetch('https://openrouter.ai/api/v1/auth/keys', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ code, code_verifier: verifier, code_challenge_method: 'S256' }),
    });
    r.httpStatus = res.status;
    const body = await res.json();
    if (!res.ok || !body.key) throw new Error(`HTTP ${res.status}: ${scrub(JSON.stringify(body))}`);
    openrouterKey = body.key;
    r.ok = true;
  } catch (e) {
    r.error = scrub(`${e.name}: ${e.message}`);
  }
  window.__results.push(r);
  log(JSON.stringify(r, null, 2));
  return r.ok;
}

$('provider').addEventListener('change', (e) => pick(e.target.value));
$('run').addEventListener('submit', (e) => {
  e.preventDefault();
  const provider = $('provider').value;
  let key = $('key').value.trim();
  $('key').value = '';
  if (provider === 'openrouter' && !key) key = openrouterKey;
  run(provider, $('model').value.trim(), $('base').value.trim().replace(/\/$/, ''), key);
});
$('signin').addEventListener('click', signIn);
$('csp').addEventListener('click', cspControl);

pick('anthropic');
window.vxrCalls = { run, cspControl, pick };

// `?signin` starts the PKCE flow straight away, so the owner's test is one link.
const params = new URLSearchParams(location.search);
const code = params.get('code');
if (params.has('signin')) signIn();
else if (code) {
  $('provider').value = 'openrouter';
  pick('openrouter');
  setStatus('OpenRouter: exchanging the code for a key');
  exchange(code).then((ok) => {
    if (ok) run('openrouter', PROVIDERS.openrouter.model, PROVIDERS.openrouter.base, openrouterKey);
    else setStatus('OpenRouter: FAIL - key exchange, see below');
  });
}
