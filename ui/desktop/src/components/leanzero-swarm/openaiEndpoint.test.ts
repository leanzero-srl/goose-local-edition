import { describe, it, expect } from 'vitest';
import {
  isOfficialOpenAiHost,
  officialEndpointReset,
  openAiEndpointOverrides,
} from './openaiEndpoint';

describe('openAiEndpointOverrides', () => {
  it('nothing saved, or the official values, is the official API', () => {
    expect(openAiEndpointOverrides([])).toEqual([]);
    expect(
      openAiEndpointOverrides([
        { key: 'OPENAI_API_KEY', value: 'sk-p...wxyz' },
        { key: 'OPENAI_HOST', value: 'https://api.openai.com' },
        { key: 'OPENAI_BASE_URL', value: 'https://eu.api.openai.com/v1' },
        { key: 'OPENAI_BASE_PATH', value: '/v1/chat/completions' },
        { key: 'OPENAI_ORGANIZATION', value: 'org-123' },
      ])
    ).toEqual([]);
  });

  it('names every field that sends the official tile elsewhere, verbatim', () => {
    expect(
      openAiEndpointOverrides([
        { key: 'OPENAI_HOST', value: 'http://localhost:1234' },
        { key: 'OPENAI_BASE_URL', value: ' https://api.openai.com.evil.test/v1 ' },
        { key: 'OPENAI_BASE_PATH', value: 'api/v0/chat/completions' },
      ])
    ).toEqual([
      { key: 'OPENAI_HOST', value: 'http://localhost:1234' },
      { key: 'OPENAI_BASE_URL', value: 'https://api.openai.com.evil.test/v1' },
      { key: 'OPENAI_BASE_PATH', value: 'api/v0/chat/completions' },
    ]);
  });

  it('an unparseable host is not the official API', () => {
    expect(isOfficialOpenAiHost('api.openai.com')).toBe(false);
    expect(openAiEndpointOverrides([{ key: 'OPENAI_HOST', value: 'not a url' }])).toEqual([
      { key: 'OPENAI_HOST', value: 'not a url' },
    ]);
  });

  it('the reset writes each overridden field back to the engine default, never a blank', () => {
    expect(
      officialEndpointReset([
        { key: 'OPENAI_BASE_URL', value: 'http://gw/v1' },
        { key: 'OPENAI_HOST', value: 'http://gw' },
      ])
    ).toEqual([
      { key: 'OPENAI_BASE_URL', value: 'https://api.openai.com/v1' },
      { key: 'OPENAI_HOST', value: 'https://api.openai.com' },
    ]);
  });
});
