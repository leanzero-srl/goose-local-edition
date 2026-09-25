import { useState, useEffect, useRef } from 'react';
import MarkdownContent from './MarkdownContent';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from './ui/collapsible';
import Expand from './ui/Expand';
import { defineMessages, useIntl } from '../i18n';

const i18n = defineMessages({
  label: { id: 'thinkingContent.label', defaultMessage: 'Thinking' },
});

interface ThinkingContentProps {
  content: string;
  isExpanded: boolean;
}

// Solid ink, upright (Q-100): the row was grey #878787 italic, the house's banned faded look, and
// what a person opens to read is body copy (DESIGN.md: ink-2 is secondary body).
export default function ThinkingContent({ content, isExpanded }: ThinkingContentProps) {
  const intl = useIntl();
  const [manualToggle, setManualToggle] = useState<boolean | null>(null);
  const prevIsExpanded = useRef(isExpanded);

  useEffect(() => {
    if (prevIsExpanded.current && !isExpanded) {
      setManualToggle(null);
    }
    prevIsExpanded.current = isExpanded;
  }, [isExpanded]);

  const expanded = manualToggle !== null ? manualToggle : isExpanded;

  return (
    <Collapsible open={expanded} onOpenChange={(open) => setManualToggle(open)} className="mb-2">
      <CollapsibleTrigger className="flex items-center gap-1.5 text-xs font-lz-medium text-lz-ink-2 hover:text-lz-ink transition-colors cursor-pointer">
        <Expand size={3} isExpanded={expanded} />
        <span>{intl.formatMessage(i18n.label)}</span>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="mt-1 ml-[18px] text-xs text-lz-ink-2">
          <MarkdownContent content={content} />
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}
