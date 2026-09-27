import { describe, expect, it, vi } from 'vitest';
import {
  ATTACHMENT_MAX_CHARS,
  REPORT_MAX_CHARS,
  contactPayload,
  mailtoUrl,
  sendProblemReport,
  systemDetailsText,
  validateReport,
} from './problemReport';
import { LEANZERO_CONTACT_API_URL } from '../branding';
import type { SystemInfo } from '../types/diagnostics';

const INFO: SystemInfo = {
  app_version: '1.9.0',
  architecture: 'aarch64',
  enabled_extensions: ['developer', 'memory'],
  model: 'qwen3.6-27b',
  os: 'macos',
  os_version: '26.6.1',
  provider: 'lmstudio',
};

const GOOD = {
  description: 'The chat froze after I sent a long message.',
  email: 'ana@example.com',
  attachment: null as string | null,
  desktopVersion: '3.0.39',
};

const reply = (status: number, body: unknown) =>
  new Response(typeof body === 'string' ? body : JSON.stringify(body), { status });

describe('validateReport (Q-192)', () => {
  it('needs 10 characters of description — the site refuses fewer', () => {
    expect(validateReport({ description: '  too short ', email: '' }, 'mail')).toBe(
      'description-short'
    );
    expect(validateReport({ description: 'x'.repeat(9), email: 'a@b.co' }, 'send')).toBe(
      'description-short'
    );
    expect(validateReport({ description: 'x'.repeat(10), email: 'a@b.co' }, 'send')).toBeNull();
  });

  it('refuses more than the field allows', () => {
    expect(
      validateReport({ description: 'x'.repeat(REPORT_MAX_CHARS + 1), email: '' }, 'mail')
    ).toBe('description-long');
  });

  it('SEND needs a reply address; the mail app does not, but a typed one must look like email', () => {
    const description = GOOD.description;
    expect(validateReport({ description, email: '' }, 'send')).toBe('email-missing');
    expect(validateReport({ description, email: '' }, 'mail')).toBeNull();
    expect(validateReport({ description, email: 'ana@' }, 'mail')).toBe('email-invalid');
    expect(validateReport({ description, email: 'ana@example' }, 'send')).toBe('email-invalid');
    expect(validateReport({ description, email: ' ana@example.com ' }, 'send')).toBeNull();
  });
});

describe('systemDetailsText — names and versions only', () => {
  it('is exactly six named lines', () => {
    expect(systemDetailsText(INFO, '3.0.39')).toBe(
      [
        'Goose Swarm desktop: 3.0.39',
        'goose engine: 1.9.0',
        'OS: macos 26.6.1 (aarch64)',
        'Provider: lmstudio',
        'Model: qwen3.6-27b',
        'Extensions: developer, memory',
      ].join('\n')
    );
  });

  it('says what is absent instead of dropping it', () => {
    const text = systemDetailsText(
      { ...INFO, provider: null, model: undefined, enabled_extensions: [] },
      ''
    );
    expect(text).toContain('Goose Swarm desktop: version not reported');
    expect(text).toContain('Provider: not set');
    expect(text).toContain('Model: not set');
    expect(text).toContain('Extensions: none');
  });

  it('a model given as a local path sends its file name, never the home folder', () => {
    for (const model of [
      '/Users/ana/models/Qwen3.6-27B-MLX-8bit',
      '~/models/Qwen3.6-27B-MLX-8bit',
      'C:\\Users\\ana\\models\\Qwen3.6-27B-MLX-8bit',
    ]) {
      const text = systemDetailsText({ ...INFO, model }, '3.0.39');
      expect(text).toContain('Model: Qwen3.6-27B-MLX-8bit');
      expect(text).not.toContain('ana');
    }
    expect(systemDetailsText({ ...INFO, model: 'qwen/qwen3.6-27b' }, '1')).toContain(
      'Model: qwen/qwen3.6-27b'
    );
  });
});

describe('mailtoUrl — the mail-app path', () => {
  it('addresses office@leanzero.net with the subject and the report, spaces as %20 not +', () => {
    const url = mailtoUrl(GOOD.description, '', null);
    expect(url.startsWith('mailto:office@leanzero.net?subject=')).toBe(true);
    const parsed = new URL(url);
    expect(parsed.pathname).toBe('office@leanzero.net');
    expect(url).not.toContain('+');
    const params = new URLSearchParams(parsed.search);
    expect(params.get('subject')).toBe('Goose Swarm problem report');
    expect(params.get('body')).toBe(GOOD.description);
  });

  it('carries the attached details and the reply address in the body', () => {
    const details = systemDetailsText(INFO, '3.0.39');
    const body = new URLSearchParams(
      new URL(mailtoUrl(GOOD.description, ' ana@example.com ', details)).search
    ).get('body')!;
    expect(body).toBe(
      `${GOOD.description}\n\n--- System details (attached by the sender) ---\n${details}\n\nReply to: ana@example.com`
    );
  });
});

describe('contactPayload — the body leanzero.net/api/contact takes', () => {
  it('names the source and never a recipient', () => {
    const payload = contactPayload({ ...GOOD, attachment: 'OS: macos' });
    expect(payload).toEqual({
      name: 'Goose Swarm user',
      email: 'ana@example.com',
      company: 'Goose Swarm 3.0.39',
      service: 'Problem report',
      message: `${GOOD.description}\n\n--- System details (attached by the sender) ---\nOS: macos`,
    });
  });
});

describe('sendProblemReport — main’s SEND path under a fake fetch', () => {
  it('posts JSON to leanzero.net/api/contact and answers ok on 200', async () => {
    const fetchImpl = vi.fn(async () => reply(200, { success: true }));
    expect(await sendProblemReport(GOOD, fetchImpl)).toEqual({ ok: true });
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe(LEANZERO_CONTACT_API_URL);
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual(contactPayload(GOOD));
  });

  it('rebuilds the body key by key — a renderer cannot add a recipient or anything else', async () => {
    const fetchImpl = vi.fn(async () => reply(200, {}));
    await sendProblemReport({ ...GOOD, to: 'someone@else.example', EMAIL_TO: 'x@y.z' }, fetchImpl);
    const sent = JSON.parse(
      (fetchImpl.mock.calls[0] as unknown as [string, RequestInit])[1].body as string
    );
    expect(Object.keys(sent).sort()).toEqual(['company', 'email', 'message', 'name', 'service']);
    expect(JSON.stringify(sent)).not.toContain('else.example');
  });

  it('refuses what the form would refuse, without touching the network', async () => {
    const fetchImpl = vi.fn();
    expect(await sendProblemReport({ ...GOOD, email: '' }, fetchImpl)).toEqual({
      ok: false,
      reason: 'invalid',
      message: 'email-missing',
    });
    expect(await sendProblemReport({ description: 1 }, fetchImpl)).toMatchObject({
      reason: 'invalid',
    });
    expect(
      await sendProblemReport(
        { ...GOOD, attachment: 'x'.repeat(ATTACHMENT_MAX_CHARS + 1) },
        fetchImpl
      )
    ).toMatchObject({ reason: 'invalid', message: 'attachment-too-long' });
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('a refusal carries the server’s own words and status', async () => {
    const notConfigured = vi.fn(async () =>
      reply(503, { error: 'Contact service is not configured' })
    );
    expect(await sendProblemReport(GOOD, notConfigured)).toEqual({
      ok: false,
      reason: 'http',
      status: 503,
      message: 'Contact service is not configured',
    });
    const html = vi.fn(async () => reply(502, '<html>Bad gateway</html>'));
    expect(await sendProblemReport(GOOD, html)).toEqual({
      ok: false,
      reason: 'http',
      status: 502,
      message: '<html>Bad gateway</html>',
    });
  });

  it('a network failure is named, not swallowed', async () => {
    const offline = vi.fn(async () => {
      throw new TypeError('fetch failed');
    });
    expect(await sendProblemReport(GOOD, offline)).toEqual({
      ok: false,
      reason: 'network',
      message: 'fetch failed',
    });
  });
});
