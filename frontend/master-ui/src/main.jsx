import React from 'react';
import { createRoot } from 'react-dom/client';
import { DesktopLifecycle } from './DesktopLifecycle.jsx';
import { ErrorBoundary } from './ErrorBoundary.jsx';
import MasterApp from './App.jsx';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <DesktopLifecycle role="Master">
      <ErrorBoundary>
        <MasterApp />
      </ErrorBoundary>
    </DesktopLifecycle>
  </React.StrictMode>
);
