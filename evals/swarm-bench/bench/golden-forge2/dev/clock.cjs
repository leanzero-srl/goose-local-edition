// A virtual clock for the golden's own bed, shared by the mock site and the app (both run in this process):
// Date.now() reads it, and every timer fast-forwards it by its delay instead of waiting — the way the
// benchmark's kit runs invocations in virtual time. advance(ms) moves the world forward between steps.
'use strict';

function installClock(startMs) {
  const realSetTimeout = global.setTimeout;
  const realNow = Date.now;
  let now = startMs;
  Date.now = () => now;
  global.setTimeout = (fn, ms, ...args) => {
    now += Math.max(0, Number(ms) || 0);
    return realSetTimeout(fn, 0, ...args);
  };
  return {
    now: () => now,
    advance: (ms) => {
      now += ms;
    },
    set: (ms) => {
      now = ms;
    },
    uninstall: () => {
      global.setTimeout = realSetTimeout;
      Date.now = realNow;
    },
  };
}

module.exports = { installClock };
