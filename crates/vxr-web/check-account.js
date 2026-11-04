async (stage, arg) => {
  // Sign-in from the editor's account popover, against `wrangler dev --local` (check runs it in
  // this order, reading each email's code and link from the wrangler log between stages):
  // 'landing' (at /about) presses the landing page's Sign in, which goes into the editor and
  // opens the popover; 'open' finds Sign in and opens the popover; 'ask' types the email and
  // presses Enter, which runs Turnstile (the always-pass test key) and has the Worker send the
  // email; 'wrong' types a wrong code; 'code' types the code from the email (arg); 'menu' opens
  // the signed-in menu; 'out' signs out; 'listen' notes the account channel; 'link' finds this
  // page signed in after the email's link was opened in another tab. 'landed', 'landed-out' and
  // 'expired' run on the account page itself (/account, Preact pre-rendered, no editor): 'landed'
  // is the tab the email's link opened (signed in, usage, the way back, the strict policy),
  // 'landed-out' presses its Sign out, 'expired' is a link that was used (?link=expired: the
  // message, the email form).
  if (stage === 'landed' || stage === 'landed-out' || stage === 'expired') {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const waitFor = async (f, ms = 8000) => { for (let t = 0; !f() && t < ms; t += 30) await sleep(30); return f(); };
    const shownId = () => document.querySelector('main > section:not([hidden])')?.id;
    const msg = () => document.getElementById('msg');
    const lines = [];
    const note = (what, ok, extra = '') => lines.push(`${ok ? 'ok' : 'FAIL'} ${what}${ok ? '' : ': ' + extra}`);
    if (stage === 'expired') {
      await waitFor(() => shownId() === 'out');
      note('a used link lands on the page with the expired message and the email form',
        shownId() === 'out' && msg().classList.contains('error') && /expired or was used/.test(msg().textContent), JSON.stringify([shownId(), msg().textContent]));
      return lines.join('\n');
    }
    if (stage === 'landed-out') {
      document.getElementById('sign-out').click();
      await waitFor(() => shownId() === 'out' && /^Signed out/.test(msg().textContent));
      note('Sign out signs out here and says projects stay in this browser', shownId() === 'out' && /^Signed out/.test(msg().textContent), JSON.stringify([shownId(), msg().textContent]));
      return lines.join('\n');
    }
    await waitFor(() => shownId() === 'in');
    const policy = (await fetch('/account', { cache: 'no-store' })).headers.get('content-security-policy') ?? '';
    note('the email\'s link lands on the account page signed in, with the way back to the editor tab',
      shownId() === 'in' && document.getElementById('who').textContent === 'As ada@example.com.' && !document.getElementById('return').hidden
        && /of \d+ MB used/.test(document.querySelector('#in .usage').textContent), JSON.stringify([shownId(), document.getElementById('who').textContent]));
    note('the page is served pre-rendered under a policy with no inline script or style',
      !!document.getElementById('root') && !!policy && !/unsafe-inline/.test(policy) && !document.querySelector('style, script:not([src])'), policy);
    return lines.join('\n');
  }
  for (let i = 0; i < 300 && !window.vxr; i++) await new Promise((r) => setTimeout(r, 50));
  const c = document.getElementById('c');
  const frames = (n = 3) => new Promise((r) => { const f = () => (--n > 0 ? requestAnimationFrame(f) : r()); requestAnimationFrame(f); });
  const until = async (f, ms = 8000) => {
    const t = performance.now();
    while (!f() && performance.now() - t < ms) await new Promise((r) => setTimeout(r, 30));
    return f();
  };
  const st = () => JSON.parse(window.vxr.state());
  const at = (name) => Array.from(window.vxr.hit(name));
  const ptr = (type, x, y) => c.dispatchEvent(new PointerEvent(type, {
    clientX: x, clientY: y, button: type === 'pointermove' ? -1 : 0, buttons: type === 'pointerdown' ? 1 : 0,
    pointerId: 1, pointerType: 'mouse', isPrimary: true, bubbles: true, cancelable: true,
  }));
  const click = async (name) => {
    const [x, y] = at(name);
    ptr('pointermove', x, y); await frames();
    ptr('pointerdown', x, y); await frames();
    ptr('pointerup', x, y); await frames();
  };
  const key = async (k, mods = {}) => {
    const o = { key: k, bubbles: true, cancelable: true, ...mods };
    c.dispatchEvent(new KeyboardEvent('keydown', o));
    c.dispatchEvent(new KeyboardEvent('keyup', o));
    await frames(2);
  };
  const type = async (text) => { for (const ch of text) await key(ch); };
  const out = [];
  const step = (what, ok, extra = '') => out.push(`${ok ? 'ok' : 'FAIL'} ${what}${ok ? '' : ': ' + extra}`);
  await frames(10);
  const shown = () => st().signIn.shown;
  if (stage === 'landing') {
    const root = document.documentElement;
    await until(() => document.getElementById('land-open')?.classList.contains('ready'), 20000);
    document.querySelector('.land-top a[href="/account"]').click();
    await until(() => 'entered' in root.dataset && shown() === 'email');
    step('the landing page\'s Sign in goes into the editor and opens the popover at its email step',
      'entered' in root.dataset && location.pathname === '/' && shown() === 'email', JSON.stringify([location.pathname, root.dataset.landing, st().signIn]));
  } else if (stage === 'open') {
    await until(() => st().account !== 'off' && at('account').length === 2);
    let s = st();
    step('with the Worker, signed out, the top bar offers Sign in', s.account === 'out' && at('account').length === 2, JSON.stringify(s.account));
    await click('scene-select');
    s = st();
    step('the projects list has no account row', s.projectsMenu && !at('projects-sign-in').length && !at('projects-account').length, JSON.stringify(s.projectsMenu));
    await key('Escape');
    await click('account');
    await until(() => shown() === 'email');
    step('Sign in opens the popover at its email step, the field focused', shown() === 'email' && window.vxr.wants_keyboard(), JSON.stringify(st().signIn));
  } else if (stage === 'ask') {
    if (shown() !== 'email') { await click('account'); await until(() => shown() === 'email'); }
    await click('account-email');
    await key('a', { metaKey: true });
    await type(arg);
    await key('Enter');
    await until(() => shown() === 'code' || st().signIn.error, 20000);
    const s = st();
    step(`Enter runs the bot check and the Worker sends the email; the popover asks for the code`, s.signIn.shown === 'code' && !s.signIn.error && !s.signIn.checking, JSON.stringify(s.signIn));
  } else if (stage === 'wrong') {
    await type('000000');
    await until(() => st().signIn.error);
    const s = st();
    step('a wrong code says so and stays on the code step', s.signIn.shown === 'code' && /doesn't match/.test(s.signIn.error ?? '') && s.account === 'out', JSON.stringify([s.signIn, s.account]));
  } else if (stage === 'code') {
    await new Promise((r) => setTimeout(r, 500)); // the wrong code's cells clear
    await type(arg);
    await until(() => shown() === 'done' || st().signIn.error);
    const done = shown() === 'done';
    await until(() => shown() === null, 4000);
    const s = st();
    step('the code from the email signs in: the check, then the popover closes and says who signed in',
      done && s.signIn.shown === null && s.account.email === 'ada@example.com' && s.toast === 'Signed in as ada@example.com', JSON.stringify([done, s.signIn, s.account, s.toast]));
  } else if (stage === 'menu') {
    await click('account');
    await until(() => shown() === 'menu');
    await frames(20);
    step('signed in, the account opens its menu: email, library use, Sign out', shown() === 'menu' && at('account-sign-out').length === 2 && at('account-library').length === 2, JSON.stringify(st().signIn));
  } else if (stage === 'out') {
    await click('account-sign-out');
    await until(() => st().account === 'out' && shown() === null);
    const s = st();
    step('Sign out signs out and closes the popover', s.account === 'out' && s.signIn.shown === null, JSON.stringify([s.account, s.signIn]));
  } else if (stage === 'listen') {
    window.__heard = 0;
    new BroadcastChannel('vxr-account').onmessage = () => window.__heard++;
    out.push('ok listening');
  } else if (stage === 'link') {
    await until(() => st().account?.email);
    await until(() => shown() === 'done' || shown() === null, 3000);
    const s = st();
    step('the link opened in another tab signs this one in (the account page says so on its channel)',
      window.__heard > 0 && s.account.email === 'ada@example.com' && (s.signIn.shown === 'done' || s.toast === 'Signed in as ada@example.com'), JSON.stringify([window.__heard, s.account, s.signIn, s.toast]));
  }
  return out.join('\n');
}
