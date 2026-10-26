import { describe, expect, it } from 'vitest';
import { controllerForm, formAnswer } from './controllerForm';

describe('controller form constraints', () => {
  it('keeps required, integer, typed enum, Unicode and exclusive bounds', () => {
    const fields = controllerForm({
      type: 'object',
      additionalProperties: false,
      required: ['count', 'label', 'enabled'],
      properties: {
        count: { type: 'integer', exclusiveMinimum: 1, maximum: 5 },
        label: { type: 'string', minLength: 1, maxLength: 1 },
        enabled: { type: 'boolean', enum: [true, false] },
        optional: { type: 'string' },
      },
    });
    expect(JSON.parse(formAnswer(fields, { count: '2', label: '🦆', enabled: '1' }))).toEqual({
      count: 2,
      label: '🦆',
      enabled: false,
    });
    expect(() => formAnswer(fields, { count: '2.5', label: 'a', enabled: '0' })).toThrow('integer');
    expect(() => formAnswer(fields, { count: '1', label: 'a', enabled: '0' })).toThrow('exceed');
    expect(() => formAnswer(fields, { count: '2', label: 'ab', enabled: '0' })).toThrow(
      'maximum length'
    );
    expect(() => formAnswer(fields, { count: '2', label: 'a' })).toThrow('required');
    expect(() => formAnswer(fields, { count: '2', label: 'a', enabled: '' })).toThrow('choose');
  });
  it.each([
    { type: 'object', properties: { key: { type: 'string', pattern: '^x' } } },
    { type: 'object', properties: { key: { type: 'array', items: { type: 'string' } } } },
    { type: 'object', properties: {}, required: ['missing'] },
    { type: 'object', properties: {}, oneOf: [] },
    { type: 'object', properties: { key: { type: 'integer', enum: ['wrong'] } } },
  ])('fails closed for unsupported or malformed schemas: %j', (schema) => {
    expect(() => controllerForm(schema)).toThrow();
  });
  it('preserves fields whose names match JavaScript prototypes', () => {
    const fields = controllerForm(
      JSON.parse(
        '{"type":"object","required":["__proto__"],"properties":{"__proto__":{"type":"string"}}}'
      )
    );
    expect(formAnswer(fields, JSON.parse('{"__proto__":"answer"}'))).toBe('{"__proto__":"answer"}');
  });
});
