// mainpage.mjs — the goose MAIN window among the app's CDP pages. Since 3.0.62 the desktop floating engine
// window (Q-224's "PiP") loads the same index.html; E2E #3l's r1 attached to that 300×159 window, found no
// "New session" button and died. The floating window's root carries data-testid="engine-glance-desktop".
export async function mainPage(browser) {
  for (const p of browser.contexts()[0].pages()) {
    if (!p.url().includes('index.html')) continue;
    const floating = await p.evaluate(() => !!document.querySelector('[data-testid=engine-glance-desktop]')).catch(() => true);
    if (!floating) return p;
  }
  throw new Error('no goose main window among the CDP pages (only the floating engine window, or none)');
}
