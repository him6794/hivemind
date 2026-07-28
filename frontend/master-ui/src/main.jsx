import React from 'react';
import { createRoot } from 'react-dom/client';
import { ErrorBoundary } from './ErrorBoundary.jsx';
import MasterApp from './App.jsx';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <ErrorBoundary>
      <MasterApp />
    </ErrorBoundary>
  </React.StrictMode>
);
