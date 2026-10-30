// The browser check's server (crates/vxr-web/check): dist/ with the headers the deploy sends
// (dist/_headers, the content security policy among them), generated fixtures under
// /__fixtures/, and a stand-in OpenAI-compatible Chat Completions endpoint under /__fake/v1
// for the assistant: a scripted provider, not a model.
//
//   node check-server.js <dist> <port> <fixtures>
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');

const [dist, port, fixtures] = process.argv.slice(2);
const types = {
  '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm',
  '.ttf': 'font/ttf', '.txt': 'text/plain', '.vox': 'application/octet-stream',
};

// Cloudflare's _headers: a path pattern, then indented "Name: value" lines.
const rules = [];
for (const line of fs.readFileSync(path.join(dist, '_headers'), 'utf8').split('\n')) {
  if (!line.trim() || line.startsWith('#')) continue;
  if (!/^\s/.test(line)) rules.push({ pattern: line.trim(), headers: {} });
  else {
    const i = line.indexOf(':');
    rules.at(-1).headers[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
}
const headersFor = (p) => Object.assign({}, ...rules
  .filter((r) => (r.pattern.endsWith('*') ? p.startsWith(r.pattern.slice(0, -1)) : p === r.pattern))
  .map((r) => r.headers));

// What the stand-in provider was sent: requests, and whether any carried a key.
const seen = { requests: 0, auth: 0, keyInBody: 0 };

function chunk(res, delta, finish = null, extra = {}) {
  const c = { id: 'standin', model: 'stand-in', object: 'chat.completion.chunk', choices: [{ index: 0, delta, finish_reason: finish }], ...extra };
  res.write(`data: ${JSON.stringify(c)}\n\n`);
}

function standIn(req, res, body) {
  seen.requests++;
  if (req.headers.authorization) seen.auth++;
  if (/sk-[a-z]/.test(body)) seen.keyInBody++;
  const msg = JSON.parse(body);
  const users = msg.messages.filter((m) => m.role === 'user');
  const last = users.at(-1)?.content ?? '';
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store' });
  const usage = { prompt_tokens: 4200, completion_tokens: 120, cost: 0.0042 };
  if (/slow/.test(last) && msg.messages.at(-1).role === 'user') {
    // A long answer, a word every 150 ms, for Stop.
    let k = 0;
    const t = setInterval(() => {
      if (res.destroyed || k >= 60) {
        clearInterval(t);
        if (!res.destroyed) { chunk(res, {}, 'stop'); res.end('data: [DONE]\n\n'); }
        return;
      }
      chunk(res, { content: `word${k++} ` });
    }, 150);
    return;
  }
  if (msg.messages.at(-1).role === 'tool') {
    chunk(res, { content: 'Done.' });
    chunk(res, { tool_calls: [{ index: 0, id: 'call_done', type: 'function', function: { name: 'done', arguments: '' } }] });
    chunk(res, { tool_calls: [{ index: 0, function: { arguments: '{"summary":"A stand-in block"}' } }] });
    chunk(res, {}, 'tool_calls');
    chunk(res, {}, null, { choices: [], usage });
    return res.end('data: [DONE]\n\n');
  }
  // Cmd-K "Organize into objects (AI)" on 23-desk: the cup and its coffee as a mug, the book,
  // and the desk that is left named.
  if (/one object per real-world thing/.test(last)) {
    const calls = [
      ['make_object', { parts: ['white', 'orange'], name: 'mug' }],
      ['make_object', { from_slot: 4, name: 'book' }],
      ['rename', { node: 1, name: 'desk' }],
    ];
    chunk(res, { role: 'assistant', content: 'A desk, a mug and a book. ' });
    calls.forEach(([name, args], index) => {
      chunk(res, { tool_calls: [{ index, id: `call_${index}`, type: 'function', function: { name, arguments: '' } }] });
      chunk(res, { tool_calls: [{ index, function: { arguments: JSON.stringify(args) } }] });
    });
    chunk(res, {}, 'tool_calls');
    chunk(res, {}, null, { choices: [], usage });
    return res.end('data: [DONE]\n\n');
  }
  // A 3x3x3 block in the top corner of the editable box, from the scene summary sent.
  const summary = JSON.parse(last.slice(last.indexOf('{'), last.lastIndexOf('\n\nRequest:')));
  const [lo, hi] = [summary.editable.from, summary.editable.to];
  const args = JSON.stringify({ from: [lo[0], hi[1] - 2, lo[2]], to: [lo[0] + 2, hi[1], lo[2] + 2], slot: 3, name: 'stand-in block' });
  res.write(': OPENROUTER PROCESSING\n\n');
  chunk(res, { role: 'assistant', content: 'Adding a block. ' });
  chunk(res, { tool_calls: [{ index: 0, id: 'call_box', type: 'function', function: { name: 'box', arguments: '' } }] });
  for (let i = 0; i < args.length; i += 16) chunk(res, { tool_calls: [{ index: 0, function: { arguments: args.slice(i, i + 16) } }] });
  chunk(res, {}, 'tool_calls');
  chunk(res, {}, null, { choices: [], usage });
  res.end('data: [DONE]\n\n');
}

http.createServer((req, res) => {
  const u = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (u === '/__fake/v1/chat/completions' && req.method === 'POST') {
    let body = '';
    req.on('data', (d) => { body += d; });
    req.on('end', () => standIn(req, res, body));
    return;
  }
  if (u === '/__fake/seen') {
    res.writeHead(200, { 'content-type': 'application/json' });
    return res.end(JSON.stringify(seen));
  }
  const p = u.startsWith('/__fixtures/') ? path.join(fixtures, u.slice(12)) : path.join(dist, u);
  const f = p.endsWith('/') ? p + 'index.html' : p;
  fs.readFile(f, (err, data) => {
    if (err) { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { ...headersFor(u), 'content-type': types[path.extname(f)] || 'application/octet-stream' });
    res.end(data);
  });
}).listen(+port, '127.0.0.1');
