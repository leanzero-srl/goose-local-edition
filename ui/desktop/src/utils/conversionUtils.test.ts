import { describe, expect, it } from 'vitest';
import { errorMessage } from './conversionUtils';

describe('provider setup error details', () => {
  it('shows the backend explanation from an SDK RequestError', () => {
    const error = Object.assign(new Error('Internal error'), {
      code: -32603,
      data: 'Google sign-in succeeded, but this account requires a Google Cloud project.',
    });
    expect(errorMessage(error)).toBe(error.data);
  });
  it('keeps normal errors and does not stringify structured error data', () => {
    expect(errorMessage(new Error('Network unavailable'))).toBe('Network unavailable');
    expect(errorMessage({ message: 'Request failed', data: { reason: 'unknown' } })).toBe(
      'Request failed'
    );
    expect(errorMessage({ message: 'Request failed', data: '  ' })).toBe('Request failed');
  });
});
