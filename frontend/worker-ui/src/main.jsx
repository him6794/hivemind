import React from 'react';
import { createRoot } from 'react-dom/client';
import { ErrorBoundary } from './ErrorBoundary.jsx';
import WorkerApp from './App.jsx';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <ErrorBoundary>
      <WorkerApp />
    </ErrorBoundary>
  </React.StrictMode>
);
