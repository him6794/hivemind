import React from 'react';
import { createRoot } from 'react-dom/client';
import MasterApp from './App.jsx';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <MasterApp />
  </React.StrictMode>
);
