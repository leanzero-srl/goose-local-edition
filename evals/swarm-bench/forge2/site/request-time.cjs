'use strict';

function requestTime(now, atMs) {
  if (atMs === undefined) return now();
  if (!Number.isFinite(atMs)) throw new TypeError('Platform request atMs must be a finite epoch millisecond number.');
  return atMs;
}

function completionTime(atMs, completedAtMs) {
  if (completedAtMs === undefined) return undefined;
  if (!Number.isFinite(completedAtMs) || completedAtMs < atMs) {
    throw new TypeError('Platform completion must be a finite epoch millisecond number at or after its send.');
  }
  return completedAtMs;
}

module.exports = { requestTime, completionTime };
