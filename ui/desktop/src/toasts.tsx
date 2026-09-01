import { X } from 'lucide-react';
import { toast, type CloseButtonProps, type ToastOptions } from 'react-toastify';
import { Button, WEIGHT } from './components/lz';
import {
  GroupedExtensionLoadingToast,
  ExtensionLoadingStatus,
} from './components/GroupedExtensionLoadingToast';

/**
 * Every toast's close: a ghost icon Button on the tokens. The container is the Studio overlay
 * surface, so the library's default close (`--text-inverse`, faded) would be white on white.
 */
function renderStudioCloseButton({ closeToast }: CloseButtonProps) {
  return (
    <Button
      variant="ghost"
      size="sm"
      iconOnly
      aria-label="Close"
      icon={<X />}
      className="self-start"
      onClick={() => closeToast()}
    />
  );
}

export interface ToastServiceOptions {
  silent?: boolean;
  shouldThrow?: boolean;
}

class ToastService {
  private silent: boolean = false;
  private shouldThrow: boolean = false;

  // Create a singleton instance
  private static instance: ToastService;

  public static getInstance(): ToastService {
    if (!ToastService.instance) {
      ToastService.instance = new ToastService();
    }
    return ToastService.instance;
  }

  configure(options: ToastServiceOptions = {}): void {
    if (options.silent !== undefined) {
      this.silent = options.silent;
    }

    if (options.shouldThrow !== undefined) {
      this.shouldThrow = options.shouldThrow;
    }
  }

  error(props: ToastErrorProps): void {
    if (!this.silent) {
      toastError(props);
    }

    if (this.shouldThrow) {
      throw new Error(props.msg);
    }
  }

  loading({ title, msg }: { title: string; msg: string }): string | number | undefined {
    if (this.silent) {
      return undefined;
    }

    const toastId = toastLoading({ title, msg });

    return toastId;
  }

  success({ title, msg }: { title: string; msg: string }): void {
    if (this.silent) {
      return;
    }
    toastSuccess({ title, msg });
  }

  dismiss(toastId?: string | number): void {
    if (toastId) toast.dismiss(toastId);
  }

  /**
   * Create a grouped extension loading toast that can be updated as extensions load
   */
  extensionLoading(
    extensions: ExtensionLoadingStatus[],
    totalCount: number,
    isComplete: boolean = false
  ): string | number {
    if (this.silent) {
      return 'silent';
    }

    const toastId = 'extension-loading';
    const hasErrors = extensions.some((ext) => ext.status === 'error');
    const autoClose = isComplete && !hasErrors ? 5000 : false;

    // Check if toast already exists
    if (toast.isActive(toastId)) {
      // Update existing toast
      toast.update(toastId, {
        render: (
          <GroupedExtensionLoadingToast
            extensions={extensions}
            totalCount={totalCount}
            isComplete={isComplete}
          />
        ),
        autoClose,
        closeButton: renderStudioCloseButton,
        closeOnClick: false,
      });
    } else {
      // Create new toast
      toast(
        <GroupedExtensionLoadingToast
          extensions={extensions}
          totalCount={totalCount}
          isComplete={isComplete}
        />,
        {
          ...commonToastOptions,
          toastId,
          autoClose,
          closeButton: renderStudioCloseButton,
          closeOnClick: false, // Prevent closing when clicking to expand/collapse
        }
      );
    }

    return toastId;
  }

  /**
   * Handle errors with consistent logging and toast notifications
   * Consolidates the functionality of the original handleError function
   */
  handleError(title: string, message: string, options: ToastServiceOptions = {}): void {
    this.configure(options);
    this.error({
      title: title,
      msg: message,
      traceback: message,
    });
  }
}

// Export a singleton instance for use throughout the app
export const toastService = ToastService.getInstance();

// Re-export ExtensionLoadingStatus for convenience
export type { ExtensionLoadingStatus };

const commonToastOptions: ToastOptions = {
  position: 'top-right',
  closeButton: renderStudioCloseButton,
  hideProgressBar: true,
  closeOnClick: true,
  pauseOnHover: true,
  draggable: true,
};

type ToastSuccessProps = { title?: string; msg?: string; toastOptions?: ToastOptions };

export function toastSuccess({ title, msg, toastOptions = {} }: ToastSuccessProps) {
  return toast.success(
    <div>
      {title ? <strong className={WEIGHT.semibold}>{title}</strong> : null}
      {title ? <div>{msg}</div> : null}
    </div>,
    { ...commonToastOptions, autoClose: 3000, ...toastOptions }
  );
}

type ToastErrorProps = {
  title: string;
  msg: string;
  traceback?: string;
  // Kept in the contract for callers; the "Ask goose" recovery button it powered was removed in
  // pass D (owner): it silently created a project-less session. Sessions start from projects only.
  recoverHints?: string;
};

function ToastErrorContent({ title, msg, traceback }: ToastErrorProps) {
  const handleCopyError = async () => {
    if (traceback) {
      try {
        await navigator.clipboard.writeText(traceback);
      } catch (error) {
        console.error('Failed to copy error:', error);
      }
    }
  };

  return (
    <div className="flex gap-4 pr-8">
      <div className="flex-grow">
        {title && <strong className={WEIGHT.semibold}>{title}</strong>}
        {msg && <div>{msg}</div>}
      </div>
      <div className="flex-none flex items-center gap-2">
        {traceback && (
          <Button size="sm" onClick={handleCopyError}>
            Copy error
          </Button>
        )}
      </div>
    </div>
  );
}

export function toastError({ title, msg, traceback }: ToastErrorProps) {
  return toast.error(<ToastErrorContent title={title} msg={msg} traceback={traceback} />, {
    ...commonToastOptions,
    autoClose: traceback ? false : 5000,
  });
}

type ToastLoadingProps = {
  title?: string;
  msg?: string;
  toastOptions?: ToastOptions;
};

export function toastLoading({ title, msg, toastOptions }: ToastLoadingProps) {
  return toast.loading(
    <div>
      {title ? <strong className={WEIGHT.semibold}>{title}</strong> : null}
      {title ? <div>{msg}</div> : null}
    </div>,
    { ...commonToastOptions, autoClose: false, ...toastOptions }
  );
}
