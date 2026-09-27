import React, { Suspense, lazy } from 'react';
import ReactDOM from 'react-dom/client';
import { IntlProvider } from 'react-intl';
import { ConfigProvider } from './components/ConfigContext';
import { ErrorBoundary } from './components/ErrorBoundary';
import SuspenseLoader from './suspense-loader';
import { applyThemeTokens } from './theme/theme-tokens';
import { applyEditionToDocument, getCachedEdition } from './contexts/EditionContext';
import { currentLocale, currentMessageLocale, loadMessages } from './i18n';
import { ThemeProvider } from './contexts/ThemeContext';

// Apply theme tokens + the Local Edition class to :root before first paint (no flash).
applyThemeTokens();
applyEditionToDocument(getCachedEdition());

const App = lazy(() => import('./App'));

let warnedFallbackLocale = false;
function handleIntlError(err: { code: string; message?: string }) {
  if (err.code === 'MISSING_TRANSLATION' && currentLocale !== currentMessageLocale) {
    if (!warnedFallbackLocale) {
      warnedFallbackLocale = true;
      console.warn(
        `[i18n] Locale "${currentLocale}" has no translations; falling back to "${currentMessageLocale}".`
      );
    }
    return;
  }
  console.error(err);
}

// The desktop engine glance (engineGlanceWindow.ts) loads this same entry at `#/engine-glance`: it
// renders only the glance card — no app, no goosed connection, no router.
const EngineGlanceDesktopRoot = lazy(() =>
  import('./components/engineGlance/EngineGlanceDesktopRoot').then((m) => ({
    default: m.EngineGlanceDesktopRoot,
  }))
);
const isEngineGlanceWindow = window.location.hash.startsWith('#/engine-glance');

(async () => {
  const messages = await loadMessages(currentMessageLocale);

  if (isEngineGlanceWindow) {
    ReactDOM.createRoot(document.getElementById('root')!).render(
      <React.StrictMode>
        <IntlProvider
          locale={currentLocale}
          defaultLocale="en"
          messages={messages}
          onError={handleIntlError}
        >
          <ThemeProvider>
            <Suspense fallback={null}>
              <EngineGlanceDesktopRoot />
            </Suspense>
          </ThemeProvider>
        </IntlProvider>
      </React.StrictMode>
    );
    return;
  }

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <IntlProvider
        locale={currentLocale}
        defaultLocale="en"
        messages={messages}
        onError={handleIntlError}
      >
        <Suspense fallback={SuspenseLoader()}>
          <ConfigProvider>
            <ErrorBoundary>
              <App />
            </ErrorBoundary>
          </ConfigProvider>
        </Suspense>
      </IntlProvider>
    </React.StrictMode>
  );
})();
