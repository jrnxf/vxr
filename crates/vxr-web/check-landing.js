// The landing page and the boot skeleton, checked in the browser by crates/vxr-web/check
// (chrome-devtools-axi eval, one stage per call so the check can screenshot between them):
//   async (stage) => lines of "ok <step>" or "FAIL <step>: <why>".
// The page is dist/ at 1280x800; the stages follow the visits of one fresh browser profile:
//   first    a first visit while the wasm is held back: the landing, with progress on its button
//   early    clicked before the editor is ready: "Opening", then the editor goes in on its own
//   ready    /about with the editor ready: the landing again, and Open the editor with no wait
//   again    a returning visit: no landing, the boot skeleton in the editor's places
//   deep     ?open= ?project= ?embed skip the landing for a browser that never saw it
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

  if (stage === 'first') {
    step('a first visit starts on the landing page', root.dataset.landing === 'on' && shown($('landing')), root.dataset.landing);
    step('the landing says what vxr is', /^Voxel scenes, ?fast\.$/.test(text(document.querySelector('#landing h1'))), text(document.querySelector('#landing h1')));
    step('the landing offers the editor and a .vox file', text($('land-open')).startsWith('Open the editor') && text($('land-file')) === 'Open a .vox file');
    step('the landing says no account is needed and where to sign in', /No account needed/.test(text(document.querySelector('.reassure'))) && !!document.querySelector('.reassure a[href="/account"]') && !!document.querySelector('.land-top a[href="/account"]'));
    step('the landing links the repository', !!document.querySelector('.land-top a[href^="https://github.com/"]'));
    step('the island picture is the editor\'s own shot', $('land-frame').firstElementChild.complete && $('land-frame').firstElementChild.naturalWidth === 1280, 'not loaded');
    step('the boot skeleton and the error message stay out of the landing', !shown($('boot')) && !shown($('msg')));
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
  } else if (stage === 'ready') {
    step('/about shows the landing again', root.dataset.landing === 'on' && shown($('landing')) && location.pathname === '/about', location.pathname + ' ' + root.dataset.landing);
    step('the button is full and says Ready once the editor is', await until(() => $('land-open').classList.contains('ready')) && $('land-load').textContent === 'Ready', $('land-load').textContent);
    step('the loaded button has no spinner showing', !$('land-open').classList.contains('opening'));
  } else if (stage === 'ready-click') {
    $('land-open').click();
    step('the island grows into the editor', await until(() => shown($('hero')) && $('hero').getAnimations().length > 0, 2000), 'no animation');
    step('the landing text has left', await until(() => getComputedStyle(document.querySelector('.land-copy')).opacity === '0', 2000));
    step('the editor opens when the picture has grown', await until(entered, 5000) && !shown($('landing')) && !$('c').inert);
    step('the address goes back to /', location.pathname === '/', location.pathname);
    step('the picture is gone', $('hero').hidden);
  } else if (stage === 'again') {
    for (let i = 0; i < 300 && !window.vxr; i++) await sleep(50);
    step('a returning visit skips the landing', root.dataset.landing === 'off' && !shown($('landing')), root.dataset.landing);
    step('the editor started', !!window.vxr && !$('c').inert);
    step('the skeleton is gone once the editor has drawn', root.hasAttribute('data-booted') && !shown($('boot')));
    // Each outline holds the editor's own part of the screen it stands in for.
    const rect = (el) => { const r = el.getBoundingClientRect(); return [r.left, r.top, r.right, r.bottom]; };
    const holds = (sk, name) => {
      const a = rect(document.querySelector(sk)), b = window.vxr.hit_rect(name);
      return b.length === 4 && b[0] >= a[0] - 1 && b[1] >= a[1] - 1 && b[2] <= a[2] + 1 && b[3] <= a[3] + 1;
    };
    for (const [sk, name] of [['.sk-title', 'scene-select'], ['.sk-dock', 'select'], ['.sk-dock', 'rail-color'], ['.sk-actions', 'ai-toggle'], ['.sk-stats', 'stats']]) {
      step(`the skeleton's ${sk.slice(4)} outline holds the editor's ${name}`, holds(sk, name), `${rect(document.querySelector(sk))} vs ${window.vxr.hit_rect(name)}`);
    }
    // Show it again for the screenshot, as it is before the first frame (the editor sets
    // data-booted on every frame, so a rule holds it open instead).
    document.styleSheets[0].insertRule('html[data-booted] #boot { opacity: 1 !important; visibility: visible !important; transition: none !important }', 0);
    step('the skeleton is six outlines and the cube', document.querySelectorAll('#boot .sk').length === 6 && !!document.querySelector('#boot .sk-mark') && getComputedStyle($('boot')).display === 'block');
  } else if (stage === 'again-done') {
    document.styleSheets[0].deleteRule(0);
  } else if (stage === 'clear') {
    try { localStorage.removeItem('vxr-visited'); } catch {}
    step('the browser forgets the visit', flag() === null, flag());
  } else if (stage === 'deep') {
    for (let i = 0; i < 300 && !window.vxr; i++) await sleep(50);
    const how = location.search;
    step(`${how} goes straight to the editor`, root.dataset.landing === 'off' && !shown($('landing')) && !!window.vxr, root.dataset.landing);
  }
  return out.join('\n');
}
