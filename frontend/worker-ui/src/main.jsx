import React from 'react';
import { createRoot } from 'react-dom/client';
import DesktopLifecycle from './DesktopLifecycle.jsx';
import { ErrorBoundary } from './ErrorBoundary.jsx';
import WorkerApp from './App.jsx';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <DesktopLifecycle role="Worker">
      <ErrorBoundary>
        <WorkerApp />
      </ErrorBoundary>
    </DesktopLifecycle>
  </React.StrictMode>
);
