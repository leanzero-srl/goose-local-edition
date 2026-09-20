import type { ChildProcess } from 'node:child_process';

type PowerBlocker = {
  start: (type: 'prevent-app-suspension') => number;
  stop: (id: number) => void;
};

/** A runner owns its assertion across build and scoring, independently of renderer windows. */
export function spawnBenchmarkWithPower<T extends ChildProcess>(power: PowerBlocker, launch: () => T) {
  const id = power.start('prevent-app-suspension');
  let held = true;
  const releasePower = () => {
    if (!held) return;
    held = false;
    power.stop(id);
  };
  try {
    const child = launch();
    child.once('error', releasePower);
    child.once('close', releasePower);
    return { child, releasePower };
  } catch (error) {
    releasePower();
    throw error;
  }
}
