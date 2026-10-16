import React from 'react';
import { createRoot } from 'react-dom/client';
import DesktopLifecycle from './DesktopLifecycle.jsx';
import { ErrorBoundary } from './ErrorBoundary.jsx';
import WorkerApp from './App.jsx';
import { LocaleProvider } from './LocaleProvider.jsx';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <LocaleProvider>
      <DesktopLifecycle role="Worker">
        <ErrorBoundary>
          <WorkerApp />
        </ErrorBoundary>
      </DesktopLifecycle>
    </LocaleProvider>
  </React.StrictMode>
);
