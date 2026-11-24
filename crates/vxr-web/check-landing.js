// The landing page and the boot screen, checked in the browser by crates/vxr-web/check
// (chrome-devtools-axi eval, one stage per call so the check can screenshot between them):
//   async (stage) => lines of "ok <step>" or "FAIL <step>: <why>".
// The page is dist/ at 1280x800; the stages follow the visits of one fresh browser profile:
//   first    a first visit while the wasm is held back: the landing, with progress on its button
//   early    clicked before the editor is ready: "Opening", then the editor goes in on its own
//   ready    /about with the editor ready: the landing again, the tour waiting behind it, and
//            Open the editor with no wait, which starts the tour
//   share    at /about: the title, the share tags and image, favicon.ico, robots.txt, the
//            sitemap, llms.txt, the 404 page, and the island fetched gzipped
//   again    a returning visit: no landing, the boot cube until the editor draws
//   deep     ?open= ?project= ?embed skip the landing and the tour for a browser that never saw them
async (stage) => {
  const out = [];
  const step = (name, pass, why = '') => out.push(pass ? `ok ${name}` : `FAIL ${name}: ${why}`);
  const root = document.documentElement;
  const $ = (id) => document.getElementById(id);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms = 20000) => { for (let t = 0; t < ms && !f(); t += 50) await sleep(50); return !!f(); };
  const shown = (el) => !!el && getComputedStyle(el).display !== 'none' && getComputedStyle(el).visibility !== 'hidden';
  const flag = () => { try { return localStorage.getItem('vxr-visited'); } catch { return 'blocked'; } };
  const entered = () => 'entered' in root.dataset;
  const text = (el) => el.textContent.replace(/\s+/g, ' ').trim();
  const tour = () => JSON.parse(window.vxr.state()).tour;

  if (stage === 'first') {
    step('a first visit starts on the landing page', root.dataset.landing === 'on' && shown($('landing')), root.dataset.landing);
    step('the landing says what vxr is', /^Voxel scenes, ?fast\.$/.test(text(document.querySelector('#landing h1'))), text(document.querySelector('#landing h1')));
    step('the landing offers the editor and a .vox file', text($('land-open')).startsWith('Open the editor') && text($('land-file')) === 'Open a .vox file');
    step('the landing says no account is needed and where to sign in', /No account needed/.test(text(document.querySelector('.reassure'))) && !!document.querySelector('.reassure a[href="/account"]') && !!document.querySelector('.land-top a[href="/account"]'));
    step('the landing links the repository', !!document.querySelector('.land-top a[href^="https://github.com/"]'));
    step('the island picture is the editor\'s own shot', $('land-frame').firstElementChild.complete && $('land-frame').firstElementChild.naturalWidth === 1280, 'not loaded');
    step('the boot screen and the error message stay out of the landing', !shown($('boot')) && !shown($('msg')));
    step('the editor behind the landing takes no keys or pointer', $('c').inert);
    // The wasm is held back by the check server, so the rest is already in and the editor is not.
    const got = await until(() => /· \d+%/.test($('land-load').textContent), 15000);
    const pct = +(/(\d+)%/.exec($('land-load').textContent)?.[1] ?? -1);
    step('the button reports real progress while the wasm loads', got && pct > 0 && pct < 100 && !root.hasAttribute('data-ready'), $('land-load').textContent);
    step('the button fills with the progress', parseFloat($('land-open').style.getPropertyValue('--p')) === pct, $('land-open').style.getPropertyValue('--p'));
  } else if (stage === 'early') {
    $('land-open').click();
    await sleep(100);
    step('clicked early, the button says Opening', $('land-open').classList.contains('opening') && !root.hasAttribute('data-ready') && !entered(), $('land-open').className);
    step('the editor opens by itself when it is ready', await until(entered, 30000), 'still on the landing');
    step('the landing is gone', !shown($('landing')) && root.dataset.landing === 'off');
    step('the picture that grew into the editor is gone', $('hero').hidden);
    step('the editor is on screen and takes the keys', !$('c').inert && root.hasAttribute('data-booted') && !!window.vxr);
    step('the browser remembers it opened the editor', flag() === '1', flag());
  } else if (stage === 'share') {
    // What a pasted link, a search engine and a mistyped address get (run at /about).
    const meta = (name) => document.querySelector(`meta[property="${name}"], meta[name="${name}"]`)?.content ?? '';
    const head = async (u) => { const r = await fetch(u, { cache: 'no-store' }); return [r.status, r.headers.get('content-type') ?? '', await r.text()]; };
    step('the page has a real title and description', /voxel scene editor/.test(document.title) && meta('description').length > 60, document.title);
    step('the page names one address for / and /about', document.querySelector('link[rel="canonical"]')?.href === 'https://vxr.jrnxf.co/');
    const image = meta('og:image');
    step('a shared link has a title, a description and a large image card',
      /voxel scene editor/.test(meta('og:title')) && meta('og:description').length > 40 && image === 'https://vxr.jrnxf.co/og.jpg' && meta('twitter:card') === 'summary_large_image', JSON.stringify([meta('og:title'), image]));
    const og = await createImageBitmap(await (await fetch('/og.jpg')).blob());
    step('the share image is 1200x630', og.width === 1200 && og.height === 630, `${og.width}x${og.height}`);
    const [ico, icoType] = await head('/favicon.ico');
    step('favicon.ico is there', ico === 200 && /icon/.test(icoType), `${ico} ${icoType}`);
    const [, , robots] = await head('/robots.txt');
    const [mapStatus, , map] = await head('/sitemap.xml');
    step('robots.txt names the sitemap, and the sitemap lists the site', /^Sitemap: https:\/\/vxr\.jrnxf\.co\/sitemap\.xml$/m.test(robots) && mapStatus === 200 && map.includes('<loc>https://vxr.jrnxf.co/</loc>'));
    const [llms, , pitch] = await head('/llms.txt');
    step('llms.txt says what vxr is', llms === 200 && /^# vxr/.test(pitch) && /not affiliated with MagicaVoxel/.test(pitch));
    const [missing, missingType, page] = await head('/no-such-page');
    step('an unknown path answers 404 with a page and a way back', missing === 404 && /html/.test(missingType) && /This page does not exist/.test(page) && /href="\/"/.test(page), `${missing} ${page.length} B`);
    const gz = performance.getEntriesByType('resource').filter((e) => /\/scene\.vox(\.gz)?$/.test(e.name)).map((e) => new URL(e.name).pathname);
    step('the island comes gzipped, never as the plain .vox', gz.length > 0 && gz.every((p) => p === '/scene.vox.gz'), JSON.stringify(gz));
    step('the gzipped island opened as the whole scene', JSON.parse(window.vxr.state()).voxels > 100000, window.vxr.state().slice(0, 80));
  } else if (stage === 'ready') {
    step('/about shows the landing again', root.dataset.landing === 'on' && shown($('landing')) && location.pathname === '/about', location.pathname + ' ' + root.dataset.landing);
    step('the button is full and says Ready once the editor is', await until(() => $('land-open').classList.contains('ready')) && $('land-load').textContent === 'Ready', $('land-load').textContent);
    step('the loaded button has no spinner showing', !$('land-open').classList.contains('opening'));
    step('the first-run tour waits behind the landing', tour().step === null && !tour().card, JSON.stringify(tour()));
  } else if (stage === 'ready-click') {
    $('land-open').click();
    step('the island grows into the editor', await until(() => shown($('hero')) && $('hero').getAnimations().length > 0, 2000), 'no animation');
    step('the landing text has left', await until(() => getComputedStyle(document.querySelector('.land-copy')).opacity === '0', 2000));
    step('the editor opens when the picture has grown', await until(entered, 5000) && !shown($('landing')) && !$('c').inert);
    step('the address goes back to /', location.pathname === '/', location.pathname);
    step('the picture is gone', $('hero').hidden);
    step('the editor opened, the first-run tour starts', await until(() => tour().step === 1 && tour().card, 2000), JSON.stringify(tour()));
  } else if (stage === 'again') {
    for (let i = 0; i < 300 && !window.vxr; i++) await sleep(50);
    step('a returning visit skips the landing', root.dataset.landing === 'off' && !shown($('landing')), root.dataset.landing);
    step('the editor started', !!window.vxr && !$('c').inert);
    step('the boot screen is gone once the editor has drawn', root.hasAttribute('data-booted') && !shown($('boot')));
    // Show it again for the screenshot, as it is before the first frame (the editor sets
    // data-booted on every frame, so a rule holds it open instead).
    document.styleSheets[0].insertRule('html[data-booted] #boot { opacity: 1 !important; visibility: visible !important; transition: none !important }', 0);
    step('the boot screen is the cube alone', document.querySelectorAll('#boot > *').length === 1 && !!document.querySelector('#boot .sk-mark') && getComputedStyle($('boot')).display === 'block');
  } else if (stage === 'again-done') {
    document.styleSheets[0].deleteRule(0);
  } else if (stage === 'clear') {
    try { localStorage.removeItem('vxr-visited'); } catch {}
    step('the browser forgets the visit', flag() === null, flag());
  } else if (stage === 'deep') {
    for (let i = 0; i < 300 && !window.vxr; i++) await sleep(50);
    const how = location.search;
    step(`${how} goes straight to the editor`, root.dataset.landing === 'off' && !shown($('landing')) && !!window.vxr, root.dataset.landing);
    await sleep(300);
    const t = tour();
    step(`${how} leaves the first-run tour out`, t.step === null && !t.card, JSON.stringify(t));
  }
  return out.join('\n');
}
