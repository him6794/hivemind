# WORKER UI

## OVERVIEW
React/Vite provider UI for worker registration and resource/status display.

## WHERE TO LOOK
- `src/App.jsx` — registration/status workflow
- `src/workerProfile.mjs` — normalized profile and registration payload
- `src/workerProfile.test.mjs` — contract tests

## RULES
Use the authenticated owner identity for registration; never trust an editable display name for ownership. Keep control API origin configurable and private by default.