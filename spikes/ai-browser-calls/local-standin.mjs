// A stand-in for Ollama's OpenAI-compatible endpoint on 127.0.0.1:11434, for the browser side of the
// local path (CSP, Chrome's Local Network Access prompt, CORS, streaming) on a machine without Ollama.
// Like Ollama it answers CORS only for origins in OLLAMA_ORIGINS (comma separated) and 127.0.0.1.
// It streams one canned `place` tool call in four chunks. It is not a model.
import http from 'node:http';

const allowed = (process.env.OLLAMA_ORIGINS || '').split(',').map((s) => s.trim()).filter(Boolean);
const ok = (origin) => !origin || /^https?:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(origin) || allowed.includes(origin);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

http.createServer(async (req, res) => {
  const origin = req.headers.origin;
  console.log(`${new Date().toISOString()} ${req.method} ${req.url} origin=${origin ?? '-'} ${ok(origin) ? 'allowed' : 'refused'}`);
  if (!ok(origin)) { res.writeHead(403); res.end(); return; }
  const cors = origin ? { 'access-control-allow-origin': origin, vary: 'Origin' } : {};
  if (req.method === 'OPTIONS') {
    res.writeHead(204, { ...cors, 'access-control-allow-methods': 'POST, GET, OPTIONS', 'access-control-allow-headers': 'authorization, content-type' });
    res.end();
    return;
  }
  if (req.method !== 'POST' || req.url !== '/v1/chat/completions') { res.writeHead(404, cors); res.end(); return; }
  for await (const _ of req);
  res.writeHead(200, { ...cors, 'content-type': 'text/event-stream' });
  const chunk = (delta, finish = null) => `data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
  const args = ['{"x":3,', '"y":1,', '"z":-2,', '"color":"#ff0000"}'];
  res.write(chunk({ role: 'assistant', tool_calls: [{ index: 0, id: 'call_0', type: 'function', function: { name: 'place', arguments: '' } }] }));
  for (const a of args) { await sleep(30); res.write(chunk({ tool_calls: [{ index: 0, function: { arguments: a } }] })); }
  res.write(chunk({}, 'tool_calls'));
  res.write('data: [DONE]\n\n');
  res.end();
}).listen(11434, '127.0.0.1', () => console.log(`stand-in on 127.0.0.1:11434, origins: ${allowed.join(' ') || '(none)'}`));
