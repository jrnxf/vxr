// Runs before the page paints (a classic script in the head): decides whether this visit starts
// on the landing page or goes straight to the editor, and says so on <html data-landing>.
// The landing shows on a first visit to "/" and always at "/about". A link that names a scene
// or a project (?open=, ?project=), an embed (?embed, or any frame) and a browser that has
// opened the editor before skip it.
(() => {
  const root = document.documentElement;
  let seen = false;
  try { seen = localStorage.getItem('vxr-visited') === '1'; } catch {}
  const q = new URLSearchParams(location.search);
  const deep = q.has('open') || q.has('project') || q.has('embed') || window.top !== window.self;
  const about = location.pathname.replace(/\/$/, '') === '/about';
  root.dataset.landing = about || (!seen && !deep) ? 'on' : 'off';
})();
