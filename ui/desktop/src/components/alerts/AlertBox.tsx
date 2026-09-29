import React, { useState, useEffect } from 'react';
import { IoIosCloseCircle, IoIosWarning, IoIosInformationCircle } from 'react-icons/io';
import { FaPencilAlt, FaSave } from 'react-icons/fa';
import { cn } from '../../utils';
import { errorMessage } from '../../utils/conversionUtils';
import { Alert, AlertType } from './types';
import { useConfig } from '../ConfigContext';
import { defineMessages, useIntl } from '../../i18n';
import { CompactionMenu } from '../compaction/CompactionMenu';

const alertIcons: Record<AlertType, React.ReactNode> = {
  [AlertType.Error]: <IoIosCloseCircle className="h-5 w-5" />,
  [AlertType.Warning]: <IoIosWarning className="h-5 w-5" />,
  [AlertType.Info]: <IoIosInformationCircle className="h-5 w-5" />,
};

interface AlertBoxProps {
  alert: Alert;
  className?: string;
  compactButtonEnabled?: boolean;
}

const i18n = defineMessages({
  autoCompactAt: {
    id: 'alertBox.autoCompactAt',
    defaultMessage: 'Auto compact at',
  },
  compactNow: {
    id: 'alertBox.compactNow',
    defaultMessage: 'Compact now',
  },
  failedToSaveThreshold: {
    id: 'alertBox.failedToSaveThreshold',
    defaultMessage: 'Couldn’t save: {error}',
  },
  context: {
    id: 'alertBox.context',
    defaultMessage: 'Context · {current} of {total} tokens · {percent}%',
  },
  contextUnknown: {
    id: 'alertBox.contextUnknown',
    defaultMessage: 'Context · {current} tokens',
  },
  editThreshold: {
    id: 'alertBox.editThreshold',
    defaultMessage: 'Change when goose compacts',
  },
  saveThreshold: {
    id: 'alertBox.saveThreshold',
    defaultMessage: 'Save',
  },
});

const alertStyles: Record<AlertType, string> = {
  [AlertType.Error]: 'bg-[#d7040e] text-white',
  [AlertType.Warning]: 'bg-[#cc4b03] text-white',
  [AlertType.Info]: 'dark:bg-white dark:text-black bg-black text-white',
};

export const AlertBox = ({ alert, className }: AlertBoxProps) => {
  const intl = useIntl();
  const { read, upsert } = useConfig();
  const [isEditingThreshold, setIsEditingThreshold] = useState(false);
  const [loadedThreshold, setLoadedThreshold] = useState<number>(0.8);
  const [thresholdValue, setThresholdValue] = useState(80);
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    const loadThreshold = async () => {
      try {
        const threshold = await read('GOOSE_AUTO_COMPACT_THRESHOLD', false);
        if (threshold !== undefined && threshold !== null && typeof threshold === 'number') {
          setLoadedThreshold(threshold);
          setThresholdValue(Math.max(1, Math.round(threshold * 100)));
        }
      } catch (err) {
        console.error('Error fetching auto-compact threshold:', err);
      }
    };

    loadThreshold();
  }, [read]);

  const currentThreshold = loadedThreshold;

  const handleSaveThreshold = async () => {
    if (isSaving) return; // Prevent double-clicks

    let validThreshold = Math.max(1, Math.min(100, thresholdValue));
    if (validThreshold !== thresholdValue) {
      setThresholdValue(validThreshold);
    }

    setIsSaving(true);
    try {
      const newThreshold = validThreshold / 100; // Convert percentage to decimal

      await upsert('GOOSE_AUTO_COMPACT_THRESHOLD', newThreshold, false);

      setIsEditingThreshold(false);
      setLoadedThreshold(newThreshold);
      setSaveError(null);

      // Notify parent component of the threshold change
      if (alert.onThresholdChange) {
        alert.onThresholdChange(newThreshold);
      }
    } catch (error) {
      // Said where the person is looking, never in a native alert (Q-357 W6).
      setSaveError(errorMessage(error, 'Unknown error'));
    } finally {
      setIsSaving(false);
    }
  };

  const box = (
    <div
      className={cn('flex flex-col gap-2 px-3 py-3', alertStyles[alert.type], className)}
      onMouseDown={(e) => {
        // Prevent popover from closing when clicking inside the alert box
        if (isEditingThreshold) {
          e.stopPropagation();
        }
      }}
    >
      {alert.progress ? (
        <div className="flex flex-col gap-2">
          <span data-testid="alert-context-line" className="text-[12px] font-semibold tnum">
            {alert.progress.total > 0
              ? intl.formatMessage(i18n.context, {
                  current: intl.formatNumber(alert.progress.current, {
                    notation: 'compact',
                    maximumFractionDigits: 1,
                  }),
                  total: intl.formatNumber(alert.progress.total, {
                    notation: 'compact',
                    maximumFractionDigits: 1,
                  }),
                  percent: Math.round((alert.progress.current / alert.progress.total) * 100),
                })
              : intl.formatMessage(i18n.contextUnknown, {
                  current: intl.formatNumber(alert.progress.current, {
                    notation: 'compact',
                    maximumFractionDigits: 1,
                  }),
                })}
          </span>
          {/* Auto-compact threshold indicator with edit */}
          <div className="flex items-center justify-center gap-1 min-h-[20px]">
            {isEditingThreshold ? (
              <>
                <span className="text-[10px]">{intl.formatMessage(i18n.autoCompactAt)}</span>
                <input
                  type="number"
                  min="1"
                  max="100"
                  step="1"
                  value={thresholdValue}
                  onChange={(e) => {
                    const val = parseInt(e.target.value, 10);
                    if (e.target.value === '') {
                      setThresholdValue(1);
                    } else if (!isNaN(val)) {
                      setThresholdValue(Math.max(1, Math.min(100, val)));
                    }
                  }}
                  onBlur={(e) => {
                    const val = parseInt(e.target.value, 10);
                    if (isNaN(val) || val < 1) {
                      setThresholdValue(1);
                    } else if (val > 100) {
                      setThresholdValue(100);
                    }
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      handleSaveThreshold();
                    } else if (e.key === 'Escape') {
                      setIsEditingThreshold(false);
                      const resetValue = Math.round(currentThreshold * 100);
                      setThresholdValue(Math.max(1, resetValue));
                    }
                  }}
                  onFocus={(e) => {
                    e.target.select();
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                  }}
                  className="w-12 px-1 text-[10px] bg-transparent border border-current rounded outline-none text-center focus:ring-1 focus:ring-current transition-colors"
                  disabled={isSaving}
                  autoFocus
                />
                <span className="text-[10px]">%</span>
                <button
                  type="button"
                  aria-label={intl.formatMessage(i18n.saveThreshold)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    handleSaveThreshold();
                  }}
                  disabled={isSaving}
                  className="p-1 rounded hover:ring-1 hover:ring-current cursor-pointer relative z-50"
                  style={{ minWidth: '20px', minHeight: '20px', pointerEvents: 'auto' }}
                >
                  <FaSave className="w-3 h-3" />
                </button>
              </>
            ) : (
              <>
                <span className="text-[10px]">
                  {intl.formatMessage(i18n.autoCompactAt)} {Math.round(currentThreshold * 100)}%
                </span>
                <button
                  type="button"
                  aria-label={intl.formatMessage(i18n.editThreshold)}
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    setIsEditingThreshold(true);
                  }}
                  className="p-1 rounded hover:ring-1 hover:ring-current cursor-pointer relative z-10"
                  style={{ minWidth: '20px', minHeight: '20px' }}
                >
                  <FaPencilAlt className="w-3 h-3" />
                </button>
              </>
            )}
          </div>
          {saveError && (
            <span
              role="alert"
              data-testid="alert-threshold-error"
              className="text-center text-[11px] font-semibold"
            >
              {intl.formatMessage(i18n.failedToSaveThreshold, { error: saveError })}
            </span>
          )}
          {alert.showCompactButton && alert.onCompact && !alert.sessionId && (
            <button
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                alert.onCompact!();
              }}
              disabled={alert.compactButtonDisabled}
              className={cn(
                'flex items-center justify-center gap-1.5 text-[11px] outline-none',
                alert.compactButtonDisabled
                  ? 'line-through cursor-not-allowed'
                  : 'hover:underline cursor-pointer'
              )}
            >
              {alert.compactIcon}
              <span>{intl.formatMessage(i18n.compactNow)}</span>
            </button>
          )}
        </div>
      ) : (
        <>
          <div className="flex items-center gap-2">
            <div className="flex-shrink-0">{alertIcons[alert.type]}</div>
            <div className="flex flex-col gap-2 flex-1">
              <span className="text-[11px] break-words whitespace-pre-line">{alert.message}</span>
              {alert.action && (
                <a
                  role="button"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    alert.action?.onClick();
                  }}
                  className="text-[11px] text-left underline hover:decoration-2 cursor-pointer outline-none"
                >
                  {alert.action.text}
                </a>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );

  // Q-357: the context meter's menu goes on to the chat's compaction — the note, Compact now, and
  // what a compaction keeps — on the surface under the coloured header.
  if (alert.progress && alert.sessionId) {
    return (
      <div className="flex flex-col">
        {box}
        <CompactionMenu
          sessionId={alert.sessionId}
          compactDisabled={alert.compactButtonDisabled === true || !alert.onCompact}
          onCompact={() => alert.onCompact?.()}
        />
      </div>
    );
  }
  return box;
};
