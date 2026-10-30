import { describe, expect, it } from 'vitest';
import { mediaToAcpContentBlock } from './prompt';

describe('attached media in the ACP prompt', () => {
  it('a named clip rides a blob resource whose URI carries its name; unnamed audio is an audio block', () => {
    expect(
      mediaToAcpContentBlock({
        kind: 'audio',
        data: 'UklGRg==',
        mimeType: 'audio/wav',
        name: 'my call.wav',
      })
    ).toEqual({
      type: 'resource',
      resource: { uri: 'attachment:///my%20call.wav', mimeType: 'audio/wav', blob: 'UklGRg==' },
    });
    expect(
      mediaToAcpContentBlock({ kind: 'audio', data: 'UklGRg==', mimeType: 'audio/wav' })
    ).toEqual({ type: 'audio', data: 'UklGRg==', mimeType: 'audio/wav' });
  });
});
