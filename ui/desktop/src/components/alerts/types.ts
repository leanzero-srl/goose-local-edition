export enum AlertType {
  Error = 'error',
  Warning = 'warning',
  Info = 'info',
}

export interface Alert {
  type: AlertType;
  message: string;
  autoShow?: boolean;
  action?: {
    text: string;
    onClick: () => void;
  };
  progress?: {
    current: number;
    total: number;
  };
  showCompactButton?: boolean;
  compactButtonDisabled?: boolean;
  onCompact?: () => void;
  compactIcon?: React.ReactNode;
  onThresholdChange?: (threshold: number) => void;
  /**
   * The chat the context meter measures (Q-357): with it, the meter's menu carries the chat's
   * compaction — the note for the next compaction, Compact now and what a compaction keeps.
   */
  sessionId?: string;
}
