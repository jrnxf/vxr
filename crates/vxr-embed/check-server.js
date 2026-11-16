// The embed check's server (crates/vxr-embed/check): the built viewer files, the generated
// scenes and the check's page from two folders, on 127.0.0.1.
//
//   node check-server.js <port> <dir> [<dir>...]
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');

const [port, ...dirs] = process.argv.slice(2);
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };

http
  .createServer((req, res) => {
    const name = path.basename(new URL(req.url, 'http://x').pathname) || 'index.html';
    const file = dirs.map((d) => path.join(d, name)).find((f) => fs.existsSync(f));
    if (!file) {
      res.writeHead(404).end('not found');
      return;
    }
    res.writeHead(200, { 'Content-Type': types[path.extname(file)] || 'application/octet-stream', 'Cache-Control': 'no-store' });
    fs.createReadStream(file).pipe(res);
  })
  .listen(+port, '127.0.0.1');
