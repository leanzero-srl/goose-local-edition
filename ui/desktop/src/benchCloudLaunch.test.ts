import { expect, it } from 'vitest';
import { validCloudEntrant } from './benchCloudLaunch';
it('accepts configured provider model namespaces and refuses malformed argv inputs', () => {
  expect(validCloudEntrant({ provider: 'custom_team', model: 'vendor/model:release' })).toBe(true);
  expect(validCloudEntrant({ provider: 'aws_bedrock', model: 'us.anthropic.model:0' })).toBe(true);
  for (const model of ['', '-flag', 'model\nother', 'model with space', 'model\0'])
    expect(validCloudEntrant({ provider: 'google', model })).toBe(false);
  expect(validCloudEntrant({ provider: '../provider', model: 'model' })).toBe(false);
});
