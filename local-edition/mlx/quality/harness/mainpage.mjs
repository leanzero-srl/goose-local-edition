// mainpage.mjs — the goose MAIN window among the app's CDP pages. Since 3.0.62 the desktop floating engine
// window (Q-224's "PiP") loads the same index.html; E2E #3l's r1 attached to that 300×159 window, found no
// "New session" button and died. The floating window loads at #/engine-glance (renderer.tsx) — the URL is the
// test: its data-testid="engine-glance-desktop" root is not rendered while the card is hidden, so on 3.0.65
// split-start took the hidden glance for the main window and navigated it off the glance route.
export async function mainPage(browser) {
  for (const p of browser.contexts()[0].pages()) {
    if (!p.url().includes('index.html')) continue;
    if (p.url().includes('#/engine-glance')) continue;
    const floating = await p.evaluate(() => !!document.querySelector('[data-testid=engine-glance-desktop]')).catch(() => true);
    if (!floating) return p;
  }
  throw new Error('no goose main window among the CDP pages (only the floating engine window, or none)');
}
