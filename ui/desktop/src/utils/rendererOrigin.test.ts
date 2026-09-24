import { describe, expect, it } from 'vitest';
import { RENDERER_ORIGIN, withRendererOrigin } from './rendererOrigin';

describe('withRendererOrigin', () => {
  it('stamps the Origin on a window request', () => {
    expect(withRendererOrigin({ Accept: '*/*' }, 1)).toEqual({
      Accept: '*/*',
      Origin: RENDERER_ORIGIN,
    });
  });

  it("leaves main's own requests without an Origin, so the Link relay does not refuse them", () => {
    const headers = { Accept: '*/*' };
    expect(withRendererOrigin(headers, undefined)).toEqual({ Accept: '*/*' });
    expect(withRendererOrigin(headers, undefined)).not.toHaveProperty('Origin');
  });
});
