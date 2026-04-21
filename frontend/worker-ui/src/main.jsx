import React from 'react';
import { createRoot } from 'react-dom/client';
import WorkerApp from './App.jsx';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <WorkerApp />
  </React.StrictMode>
);
