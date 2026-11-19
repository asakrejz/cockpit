// Layout check for the floating navigation (repeatable, run in the Browser pane).
//
// What it checks, per route, at the viewport you have set (375, 768 and 1280 px wide):
//   1. After scrolling every scroller to its end, the last element is fully visible above the
//      navigation pill and is the element a click at its centre would hit.
//   2. The pill is shown at the end of the scroll.
//   3. No button, link or field that is on screen and above the pill is covered by it, and the
//      full-width wrapper of the pill takes no clicks outside of the pill.
//
// How to run (never against real services; use a test server with COCKPIT_SEED=demo, see README):
//   1. Start the test server (own COCKPIT_ENV_FILE, COCKPIT_CONFIG, COCKPIT_DB, port other than 8787).
//   2. Open it in the Browser pane, emulate the width (resize_window 375x812, 768x1024, 1280x800).
//   3. Paste this file into `javascript_tool` and finish with:  await cockpitLayoutCheck()
//   The result is a list of problems; an empty list means everything is fine.
async function cockpitLayoutCheck(routes) {
  routes = routes || [
    '#/', '#/mine', '#/team', '#/snoozed', '#/overview', '#/review', '#/agent',
    '#/log', '#/limits', '#/notifications', '#/setup',
  ];
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const problems = [];
  const describe = (el) =>
    el.tagName.toLowerCase() + (el.className && typeof el.className === 'string' ? '.' + el.className.split(' ')[0] : '') +
    ' "' + (el.textContent || '').trim().slice(0, 30) + '"';
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none';
  };
  // Elements scrolled out of their scroller are not on screen and do not count.
  const clipped = (el, box) => { const r = el.getBoundingClientRect(); return r.bottom > box.top && r.top < box.bottom; };
  const scrollers = () => {
    const found = [document.scrollingElement];
    for (const el of document.querySelectorAll('.ui-pane, .ck-setup')) {
      const oy = getComputedStyle(el).overflowY;
      if ((oy === 'auto' || oy === 'scroll') && el.scrollHeight > el.clientHeight + 1) found.push(el);
    }
    return found;
  };
  for (const route of routes) {
    location.hash = route;
    await wait(500);
    const where = route + ' @' + innerWidth;
    const wrapper = document.querySelector('.ui-bottomnav');
    const pill = document.querySelector('.ui-bottomnav__pill');
    if (!pill) { problems.push(where + ': no navigation pill'); continue; }
    // 3. the wrapper only positions
    if (getComputedStyle(wrapper).pointerEvents !== 'none') problems.push(where + ': wrapper takes clicks');
    for (const scroller of scrollers()) {
      // On wide screens the page itself does not scroll (the panes do).
      if (scroller === document.scrollingElement && scroller.scrollHeight <= innerHeight + 1) continue;
      scroller.scrollTop = scroller.scrollHeight;
      await wait(450);
      const pillRect = pill.getBoundingClientRect();
      // 2. the pill is back at the end
      if (wrapper.hasAttribute('data-hidden')) problems.push(where + ': pill hidden at the end of the scroll');
      const box = scroller === document.scrollingElement
        ? { top: 0, bottom: innerHeight }
        : scroller.getBoundingClientRect();
      // 1. the last element
      let last = null;
      for (const el of scroller.querySelectorAll('*')) {
        if (el.closest('.ui-bottomnav, .ui-bottomnav-overlay, .ck-toasts, .ck-banner-stack')) continue;
        if (!visible(el) || !clipped(el, box) || (el.children.length && !el.matches('button, a, input, select, textarea, summary'))) continue;
        const r = el.getBoundingClientRect();
        if (!last || r.bottom > last.rect.bottom) last = { el, rect: r };
      }
      if (last) {
        const limit = Math.min(box.bottom, pillRect.top);
        if (last.rect.bottom > limit + 0.5)
          problems.push(where + ': last element ' + describe(last.el) + ' ends at ' + Math.round(last.rect.bottom) + ', pill starts at ' + Math.round(pillRect.top));
        const x = last.rect.left + last.rect.width / 2;
        const y = last.rect.top + last.rect.height / 2;
        if (y > 0 && y < innerHeight) {
          const hit = document.elementFromPoint(x, y);
          if (hit && pill.contains(hit)) problems.push(where + ': last element ' + describe(last.el) + ' is covered by the pill');
        }
      }
      // 3. controls under the pill (at the end of the scroll nothing may be there)
      for (const el of scroller.querySelectorAll('button, a[href], input, select, textarea, summary')) {
        if (el.closest('.ui-bottomnav, .ui-bottomnav-overlay') || !visible(el) || !clipped(el, box)) continue;
        const r = el.getBoundingClientRect();
        const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
        if (cy < 0 || cy > innerHeight) continue;
        const hit = document.elementFromPoint(cx, cy);
        if (hit && pill.contains(hit)) problems.push(where + ': ' + describe(el) + ' lies under the pill');
      }
      scroller.scrollTop = 0;
    }
    // 3. the strip left and right of the pill is no click target of the navigation
    const strip = document.elementFromPoint(2, pillRect0(pill).top + pillRect0(pill).height / 2);
    if (strip && strip.closest('.ui-bottomnav')) problems.push(where + ': the strip beside the pill takes clicks');
  }
  function pillRect0(p) { return p.getBoundingClientRect(); }
  return { width: innerWidth, routes: routes.length, problems };
}
