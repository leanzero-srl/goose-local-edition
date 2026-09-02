import type { ReactElement, ReactNode } from 'react';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CloseButtonProps, ToastOptions } from 'react-toastify';
import { assertStudioClean } from './components/lz/assertStudioClean';

const mocks = vi.hoisted(() => ({
  toast: Object.assign(vi.fn(), {
    isActive: vi.fn(() => false),
    update: vi.fn(),
    dismiss: vi.fn(),
    success: vi.fn(),
    error: vi.fn(),
    loading: vi.fn(),
  }),
}));
vi.mock('react-toastify', () => ({ toast: mocks.toast }));
vi.mock('./components/GroupedExtensionLoadingToast', () => ({
  GroupedExtensionLoadingToast: () => null,
}));

import {
  renderStudioCloseButton,
  toastError,
  toastLoading,
  toastService,
  toastSuccess,
} from './toasts';

type CloseRender = (props: CloseButtonProps) => ReactNode;

function renderClose(options: ToastOptions) {
  const closeToast = vi.fn();
  const render_ = options.closeButton as CloseRender;
  const { container } = render(<>{render_({ closeToast, type: 'default', theme: 'light' })}</>);
  return { closeToast, container };
}

/**
 * The toast surface is the Studio overlay (App's ToastContainer). These pin the parts this
 * module owns: a ghost icon Button closes every toast (the library default is
 * `--text-inverse`, faded — invisible on the light surface), and the content is on the tokens.
 */
describe('toasts (Studio)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.toast.isActive.mockReturnValue(false);
  });

  it('the extension-loading toast closes through a ghost icon Button, on create and on update', () => {
    toastService.extensionLoading([], 2, false);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    const created = mocks.toast.mock.calls[0][1] as ToastOptions;
    const { closeToast, container } = renderClose(created);
    const close = screen.getByRole('button', { name: 'Close' });
    expect(close.getAttribute('data-variant')).toBe('ghost');
    expect(close.getAttribute('data-icon-only')).toBe('true');
    expect(close.className).toContain('size-7');
    fireEvent.click(close);
    expect(closeToast).toHaveBeenCalledTimes(1);
    assertStudioClean(container);

    mocks.toast.isActive.mockReturnValue(true);
    toastService.extensionLoading([], 2, true);
    expect(mocks.toast.update).toHaveBeenCalledTimes(1);
    const updated = mocks.toast.update.mock.calls[0][1] as ToastOptions;
    expect(updated.closeButton).toBe(created.closeButton);
  });

  it('success and error toasts share the same Studio close', () => {
    toastSuccess({ title: 'Saved', msg: 'ok' });
    toastError({ title: 'Boom', msg: 'It broke' });
    const success = mocks.toast.success.mock.calls[0][1] as ToastOptions;
    const error = mocks.toast.error.mock.calls[0][1] as ToastOptions;
    expect(typeof success.closeButton).toBe('function');
    expect(error.closeButton).toBe(success.closeButton);
  });

  it('the error toast copies through a secondary lz Button and sets its title in the semibold weight', () => {
    toastError({ title: 'Boom', msg: 'It broke', traceback: 'stack' });
    const content = mocks.toast.error.mock.calls[0][0] as ReactElement;
    const { container } = render(content);
    const copy = screen.getByRole('button', { name: 'Copy error' });
    expect(copy.getAttribute('data-variant')).toBe('secondary');
    expect(copy.className).not.toMatch(/bg-background-inverse|opacity/);
    expect(screen.getByText('Boom').className).toContain('font-lz-semibold');
    assertStudioClean(container);
  });

  it('success, loading and error content carry a StatusDot for the tone — the container renders no library icon', () => {
    toastSuccess({ title: 'Saved', msg: 'ok' });
    toastLoading({ title: 'Working', msg: 'on it' });
    toastError({ title: 'Boom', msg: 'It broke' });
    const success = render(mocks.toast.success.mock.calls[0][0] as ReactElement).container;
    const loading = render(mocks.toast.loading.mock.calls[0][0] as ReactElement).container;
    const error = render(mocks.toast.error.mock.calls[0][0] as ReactElement).container;
    expect(within(success).getByRole('img', { name: 'Success' }).className).toContain('bg-lz-ok');
    expect(within(success).getByText('Saved').className).toContain('font-lz-semibold');
    const live = within(loading).getByRole('img', { name: 'In progress' });
    expect(live.getAttribute('data-live')).toBe('true');
    expect(live.className).toContain('bg-lz-accent');
    expect(within(error).getByRole('img', { name: 'Error' }).className).toContain('bg-lz-err');
    for (const c of [success, loading, error]) assertStudioClean(c);
  });

  it('the Studio close renderer App mounts on the ToastContainer is the one every toast option names', () => {
    toastSuccess({ title: 'Saved', msg: 'ok' });
    const success = mocks.toast.success.mock.calls[0][1] as ToastOptions;
    expect(success.closeButton).toBe(renderStudioCloseButton);
  });
});
