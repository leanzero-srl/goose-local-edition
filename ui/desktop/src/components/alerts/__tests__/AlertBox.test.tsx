import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, render, type RenderOptions, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { AlertBox } from '../AlertBox';
import { Alert, AlertType } from '../types';
import { IntlTestWrapper } from '../../../i18n/test-utils';

const renderWithIntl = (ui: React.ReactElement, options?: RenderOptions) =>
  render(ui, { wrapper: IntlTestWrapper, ...options });

// Mock the ConfigContext
const config = vi.hoisted(() => ({ read: vi.fn(), upsert: vi.fn() }));
vi.mock('../../ConfigContext', () => ({
  useConfig: () => config,
}));

const acp = vi.hoisted(() => ({ compactionPreview: vi.fn(), compactionSteer: vi.fn() }));
vi.mock('../../../acp/compaction', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../acp/compaction')>()),
  ...acp,
}));

describe('AlertBox', () => {
  const mockOnCompact = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    config.read.mockResolvedValue(0.8);
  });

  describe('Basic Rendering', () => {
    it('should render info alert with message', () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Test info message',
      };

      renderWithIntl(<AlertBox alert={alert} />);

      expect(screen.getByText('Test info message')).toBeInTheDocument();
    });

    it('should render warning alert with correct styling', () => {
      const alert: Alert = {
        type: AlertType.Warning,
        message: 'Test warning message',
      };

      const { container } = renderWithIntl(<AlertBox alert={alert} />);
      const alertElement = container.querySelector('.bg-\\[\\#cc4b03\\]');

      expect(alertElement).toBeInTheDocument();
      expect(screen.getByText('Test warning message')).toBeInTheDocument();
    });

    it('should render error alert with correct styling', () => {
      const alert: Alert = {
        type: AlertType.Error,
        message: 'Test error message',
      };

      const { container } = renderWithIntl(<AlertBox alert={alert} />);
      const alertElement = container.querySelector('.bg-\\[\\#d7040e\\]');

      expect(alertElement).toBeInTheDocument();
      expect(screen.getByText('Test error message')).toBeInTheDocument();
    });

    it('should apply custom className', () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Test message',
      };

      const { container } = renderWithIntl(<AlertBox alert={alert} className="custom-class" />);
      const alertElement = container.firstChild as HTMLElement;

      expect(alertElement).toHaveClass('custom-class');
    });
  });

  describe('Progress Alert', () => {
    it('should render auto-compact threshold when progress is provided', async () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: {
          current: 50,
          total: 100,
        },
      };

      renderWithIntl(<AlertBox alert={alert} />);

      // Should show auto-compact threshold (default 80%)
      expect(await screen.findByText(/Auto compact at 80%/)).toBeInTheDocument();
    });

    it('should not render progress dots or token counts', () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: {
          current: 1500,
          total: 10000,
        },
      };

      const { container } = renderWithIntl(<AlertBox alert={alert} />);

      // Progress dots and token counts are no longer rendered
      expect(screen.queryByText('1.5k')).not.toBeInTheDocument();
      expect(screen.queryByText('10k')).not.toBeInTheDocument();
      expect(screen.queryByText('15%')).not.toBeInTheDocument();
      const progressDots = container.querySelectorAll('.h-\\[2px\\]');
      expect(progressDots.length).toBe(0);
    });
  });

  describe('Compact Button', () => {
    it('should render compact button when showCompactButton is true', () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: { current: 50, total: 100 },
        showCompactButton: true,
        onCompact: mockOnCompact,
      };

      renderWithIntl(<AlertBox alert={alert} />);

      expect(screen.getByText('Compact now')).toBeInTheDocument();
    });

    it('should render compact button with custom icon', () => {
      const CompactIcon = () => <span data-testid="compact-icon">📦</span>;

      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: { current: 50, total: 100 },
        showCompactButton: true,
        onCompact: mockOnCompact,
        compactIcon: <CompactIcon />,
      };

      renderWithIntl(<AlertBox alert={alert} />);

      expect(screen.getByTestId('compact-icon')).toBeInTheDocument();
      expect(screen.getByText('Compact now')).toBeInTheDocument();
    });

    it('should call onCompact when compact button is clicked', async () => {
      const user = userEvent.setup();

      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: { current: 50, total: 100 },
        showCompactButton: true,
        onCompact: mockOnCompact,
      };

      renderWithIntl(<AlertBox alert={alert} />);

      const compactButton = screen.getByText('Compact now');
      await user.click(compactButton);

      expect(mockOnCompact).toHaveBeenCalledTimes(1);
    });

    it('should prevent event propagation when compact button is clicked', () => {
      const mockParentClick = vi.fn();

      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: { current: 50, total: 100 },
        showCompactButton: true,
        onCompact: mockOnCompact,
      };

      renderWithIntl(
        <div onClick={mockParentClick}>
          <AlertBox alert={alert} />
        </div>
      );

      const compactButton = screen.getByText('Compact now');
      fireEvent.click(compactButton);

      expect(mockOnCompact).toHaveBeenCalledTimes(1);
      expect(mockParentClick).not.toHaveBeenCalled();
    });

    it('should not render compact button when showCompactButton is false', () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: { current: 50, total: 100 },
        showCompactButton: false,
        onCompact: mockOnCompact,
      };

      renderWithIntl(<AlertBox alert={alert} />);

      expect(screen.queryByText('Compact now')).not.toBeInTheDocument();
    });

    it('should not render compact button when onCompact is not provided', () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: { current: 50, total: 100 },
        showCompactButton: true,
      };

      renderWithIntl(<AlertBox alert={alert} />);

      expect(screen.queryByText('Compact now')).not.toBeInTheDocument();
    });
  });

  describe('Combined Features', () => {
    it('should render threshold settings and compact button together', async () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: {
          current: 75,
          total: 100,
        },
        showCompactButton: true,
        onCompact: mockOnCompact,
      };

      renderWithIntl(<AlertBox alert={alert} />);

      expect(await screen.findByText(/Auto compact at 80%/)).toBeInTheDocument();
      expect(screen.getByText('Compact now')).toBeInTheDocument();
    });

    it('should handle multiline messages', () => {
      const alert: Alert = {
        type: AlertType.Warning,
        message: 'Line 1\nLine 2\nLine 3',
      };

      renderWithIntl(<AlertBox alert={alert} />);

      expect(
        screen.getByText(
          (content) =>
            content.includes('Line 1') && content.includes('Line 2') && content.includes('Line 3')
        )
      ).toBeInTheDocument();
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty message', () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: '',
      };

      const { container } = renderWithIntl(<AlertBox alert={alert} />);

      const alertElement = container.querySelector('.flex.flex-col.gap-2');
      expect(alertElement).toBeInTheDocument();
    });

    it('should handle progress with zero total gracefully', async () => {
      const alert: Alert = {
        type: AlertType.Info,
        message: 'Context window',
        progress: {
          current: 10,
          total: 0,
        },
      };

      renderWithIntl(<AlertBox alert={alert} />);

      // Should still render threshold settings
      expect(await screen.findByText(/Auto compact at 80%/)).toBeInTheDocument();
    });
  });

  describe('Q-357: the context meter menu', () => {
    const meter: Alert = {
      type: AlertType.Warning,
      message: 'Context window',
      progress: { current: 142_100, total: 178_200 },
      showCompactButton: true,
      onCompact: mockOnCompact,
    };

    it('says the context in one line', () => {
      renderWithIntl(<AlertBox alert={meter} />);
      expect(screen.getByTestId('alert-context-line').textContent).toBe(
        'Context · 142.1K of 178.2K tokens · 80%'
      );
    });

    it('a threshold that could not be saved is said inline, never in a native alert', async () => {
      const nativeAlert = vi.spyOn(window, 'alert').mockImplementation(() => undefined);
      config.upsert.mockRejectedValue(new Error('config.yaml is read-only'));
      renderWithIntl(<AlertBox alert={meter} />);
      await screen.findByText(/Auto compact at 80%/);
      fireEvent.click(screen.getByRole('button', { name: 'Change when goose compacts' }));
      fireEvent.mouseDown(screen.getByRole('button', { name: 'Save' }));
      expect((await screen.findByTestId('alert-threshold-error')).textContent).toBe(
        'Couldn’t save: config.yaml is read-only'
      );
      expect(nativeAlert).not.toHaveBeenCalled();
    });

    it('with the chat, carries its note, Compact now and what a compaction keeps', async () => {
      acp.compactionPreview.mockResolvedValue({
        kept: [],
        writtenParts: [],
        alwaysHere: { ledgerTail: [] },
        steer: { note: 'keep the 24-month rule', standing: true, pins: [] },
      });
      acp.compactionSteer.mockImplementation(async (_: string, steer: unknown) => steer);
      renderWithIntl(<AlertBox alert={{ ...meter, sessionId: 's1' }} />);
      const note = (await screen.findByDisplayValue(
        'keep the 24-month rule'
      )) as HTMLTextAreaElement;
      expect(screen.getByTestId('compaction-menu-standing')).toHaveAttribute(
        'aria-checked',
        'true'
      );
      fireEvent.change(note, { target: { value: 'use the 12-month cutoff' } });
      await act(async () => {
        fireEvent.click(screen.getByTestId('compaction-menu-compact'));
      });
      expect(acp.compactionSteer).toHaveBeenCalledWith(
        's1',
        expect.objectContaining({ note: 'use the 12-month cutoff', standing: true })
      );
      expect(mockOnCompact).toHaveBeenCalledTimes(1);
      expect(screen.queryByText('Compact now', { selector: 'span' })).toBeNull();
    });
  });
});
