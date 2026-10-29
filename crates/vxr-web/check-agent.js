// The browser check's terminal agent (crates/vxr-web/check): runs `<vxr> mcp --live` as Claude
// Code would (MCP on its stdin and stdout), makes the calls given as JSON, and prints each
// call's result as one JSON line (with its picture's type, for look). A string entry "wait <file>" waits for that file first, so
// the check can act in the page between two calls of the same server.
//
//   node check-agent.js <vxr binary> '[["scene_summary", {}], "wait /tmp/go", ["box", {...}]]'
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const readline = require('node:readline');

const [bin, calls] = [process.argv[2], JSON.parse(process.argv[3])];
const server = spawn(bin, ['mcp', '--live'], { stdio: ['pipe', 'pipe', 'inherit'] });
const waiting = new Map();
readline.createInterface({ input: server.stdout }).on('line', (line) => {
  const m = JSON.parse(line);
  waiting.get(m.id)?.(m);
});
let next = 0;
const write = (m) => server.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...m }) + '\n');
const rpc = (method, params) => new Promise((resolve) => {
  const id = ++next;
  waiting.set(id, resolve);
  write({ id, method, params });
});

(async () => {
  await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'claude-code', version: '1' } });
  write({ method: 'notifications/initialized' });
  for (const c of calls) {
    if (typeof c === 'string') {
      const file = c.replace(/^wait /, '');
      while (!fs.existsSync(file)) await new Promise((r) => setTimeout(r, 50));
      continue;
    }
    const r = await rpc('tools/call', { name: c[0], arguments: c[1] });
    const image = r.result?.content?.[1];
    const picture = image?.type === 'image' ? { mimeType: image.mimeType, png: Buffer.from(image.data, 'base64').subarray(1, 4).toString() } : null;
    console.log(JSON.stringify({ tool: c[0], text: r.result?.content?.[0]?.text ?? '', isError: r.result?.isError ?? true, image: picture }));
  }
  server.stdin.end();
})();
