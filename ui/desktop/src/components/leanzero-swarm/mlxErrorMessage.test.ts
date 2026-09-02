import { describe, expect, it } from 'vitest';
import { mlxErrorMessage } from './mlxErrorMessage';

// The exact rejection measured on 2026-09-02: the SDK's RequestError shape (an Error carrying
// code + data), where `data` is the sidecar's reason and `message` is only the JSON-RPC class.
const REASON = 'port 8090 has an unsupervised listener — unmount/reclaim it first';
const measured = Object.assign(new Error('Invalid params'), { code: -32602, data: REASON });

describe('mlxErrorMessage — the sidecar reason outranks the JSON-RPC class', () => {
  it('prefers the data string of the measured ACP error over its message', () => {
    expect(mlxErrorMessage(measured, 'Mount failed.')).toBe(REASON);
  });

  it('reads the same shape as a plain object (the wire form)', () => {
    expect(mlxErrorMessage({ code: -32602, message: 'Invalid params', data: REASON }, 'x')).toBe(
      REASON
    );
  });

  it('falls back to message when data is absent, empty, or not a string', () => {
    expect(mlxErrorMessage(new Error('model directory is incomplete'), 'x')).toBe(
      'model directory is incomplete'
    );
    expect(mlxErrorMessage(Object.assign(new Error('Invalid params'), { data: '   ' }), 'x')).toBe(
      'Invalid params'
    );
    expect(
      mlxErrorMessage(Object.assign(new Error('Invalid params'), { data: { port: 8090 } }), 'x')
    ).toBe('Invalid params');
  });

  it('keeps the caller fallback for non-error rejections', () => {
    expect(mlxErrorMessage(undefined, 'Mount failed.')).toBe('Mount failed.');
  });
});
