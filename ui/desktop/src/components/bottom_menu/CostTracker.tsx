import { useState, useEffect } from 'react';
import { CoinIcon } from '../icons';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/Tooltip';
import { fetchCanonicalModelInfo, type CanonicalModelInfo } from '../../utils/canonical';
import { defineMessages, useIntl } from '../../i18n';
import { Chip, MOTION, TNUM, cx } from '../lz';
import { isLocalProviderName } from '../settings/models/leanzeroSelectorPolicy';

const i18n = defineMessages({
  pricingUnavailable: {
    id: 'costTracker.pricingUnavailable',
    defaultMessage: 'Pricing data unavailable for {model}',
  },
  totalSessionCost: {
    id: 'costTracker.totalSessionCost',
    defaultMessage: 'Total session cost: {cost}',
  },
  inputOutputTooltip: {
    id: 'costTracker.inputOutputTooltip',
    defaultMessage:
      'Input: {inputTokens} tokens ({inputCost}) | Output: {outputTokens} tokens ({outputCost})',
  },
});

interface CostTrackerProps {
  inputTokens?: number;
  outputTokens?: number;
  accumulatedCost?: number | null;
  model: string | null;
  provider: string | null;
}

/**
 * The session's cost, in its own chip — or NOTHING. A price is shown only when there is one: a
 * local provider (swarm, the LeanZero MLX engine, LM Studio, Ollama …) has no per-token price,
 * and a cloud model with no known pricing has an unknown cost; both used to read "0.0000", a
 * fabricated number (UX audit C8). The chip lives here, not at the call site, so a null readout can
 * never leave an empty chip behind.
 */
export function CostTracker({
  inputTokens = 0,
  outputTokens = 0,
  accumulatedCost,
  model: currentModel,
  provider: currentProvider,
}: CostTrackerProps) {
  const intl = useIntl();
  const [costInfo, setCostInfo] = useState<CanonicalModelInfo | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [showPricing, setShowPricing] = useState(true);
  const [pricingFailed, setPricingFailed] = useState(false);

  // Check if pricing is enabled
  useEffect(() => {
    const loadPricingSetting = async () => {
      const enabled = await window.electron.getSetting('showPricing');
      setShowPricing(enabled);
    };

    loadPricingSetting();

    const handlePricingChange = () => {
      loadPricingSetting();
    };

    window.addEventListener('showPricingChanged', handlePricingChange);
    return () => window.removeEventListener('showPricingChanged', handlePricingChange);
  }, []);

  const local = currentProvider != null && isLocalProviderName(currentProvider);

  useEffect(() => {
    const loadCostInfo = async () => {
      if (!currentModel || !currentProvider || local) {
        setIsLoading(false);
        return;
      }

      setIsLoading(true);
      try {
        const costData = await fetchCanonicalModelInfo(currentProvider, currentModel);
        if (costData) {
          setCostInfo(costData);
          setPricingFailed(false);
        } else {
          setPricingFailed(true);
          setCostInfo(null);
        }
      } catch {
        setPricingFailed(true);
        setCostInfo(null);
      } finally {
        setIsLoading(false);
      }
    };

    loadCostInfo();
  }, [currentModel, currentProvider, local]);

  // Return null early if pricing is disabled
  if (!showPricing) {
    return null;
  }

  if (local) {
    return null;
  }

  const calculateCost = (): number => {
    return accumulatedCost ?? 0;
  };

  const formatCost = (cost: number): string => cost.toFixed(2);

  // Show loading state or when we don't have model/provider info
  if (!currentModel || !currentProvider) {
    return null;
  }

  if (isLoading) {
    return null;
  }

  if (
    accumulatedCost == null &&
    (!costInfo || (costInfo.inputTokenCost === undefined && costInfo.outputTokenCost === undefined))
  ) {
    return null;
  }

  const totalCost = calculateCost();

  // Build tooltip content
  const getTooltipContent = (): string => {
    if (pricingFailed) {
      return intl.formatMessage(i18n.pricingUnavailable, {
        model: `${currentProvider}/${currentModel}`,
      });
    }

    const currency = costInfo?.currency || '$';

    if (accumulatedCost != null) {
      return (
        intl.formatMessage(i18n.totalSessionCost, { cost: `${currency}${totalCost.toFixed(4)}` }) +
        `\n` +
        intl.formatMessage(i18n.inputOutputTooltip, {
          inputTokens: inputTokens.toLocaleString(),
          inputCost: `${currency}${((inputTokens * (costInfo?.inputTokenCost || 0)) / 1_000_000).toFixed(6)}`,
          outputTokens: outputTokens.toLocaleString(),
          outputCost: `${currency}${((outputTokens * (costInfo?.outputTokenCost || 0)) / 1_000_000).toFixed(6)}`,
        })
      );
    }

    const inputCostStr = `${currency}${((inputTokens * (costInfo?.inputTokenCost || 0)) / 1_000_000).toFixed(6)}`;
    const outputCostStr = `${currency}${((outputTokens * (costInfo?.outputTokenCost || 0)) / 1_000_000).toFixed(6)}`;
    return intl.formatMessage(i18n.inputOutputTooltip, {
      inputTokens: inputTokens.toLocaleString(),
      inputCost: inputCostStr,
      outputTokens: outputTokens.toLocaleString(),
      outputCost: outputCostStr,
    });
  };

  return (
    <Chip>
      <Tooltip>
        <TooltipTrigger asChild>
          <div
            data-testid="cost-readout"
            className={cx(
              'flex h-full cursor-default items-center justify-center text-lz-ink-3 hover:text-lz-ink',
              MOTION
            )}
          >
            <CoinIcon className="mr-1" size={16} />
            <span className={cx('text-lz-meta', TNUM)}>{formatCost(totalCost)}</span>
          </div>
        </TooltipTrigger>
        <TooltipContent>{getTooltipContent()}</TooltipContent>
      </Tooltip>
    </Chip>
  );
}
