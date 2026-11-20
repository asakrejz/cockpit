// Overflow check: paste into the browser console (or run via the browser tool) on the playground
// story `#extreme` or on the app. Asserts that no pane and no page is wider than its box.
(() => {
  const bad = [];
  const doc = document.documentElement;
  if (doc.scrollWidth > doc.clientWidth) bad.push(`page ${doc.scrollWidth} > ${doc.clientWidth}`);
  document.querySelectorAll('.ui-pane, .ui-card, .ui-list__row, .ui-inbox-row, .ui-dialog, [data-extreme-pane]').forEach((el) => {
    const code = getComputedStyle(el).overflowX;
    if (el.scrollWidth > el.clientWidth + 1 && code !== 'auto' && code !== 'scroll') {
      bad.push(`${el.className} ${el.scrollWidth} > ${el.clientWidth}`);
    }
  });
  console.log(bad.length ? `OVERFLOW: ${bad.join('; ')}` : 'no overflow');
  return bad;
})();
