# MASTER UI

## OVERVIEW
React/Vite administration UI for authenticated task and worker operations.

## WHERE TO LOOK
- `src/App.jsx` — user workflow and API calls
- `src/taskIdPolicy.mjs` — task identifier boundary
- `src/artifactDownloadPolicy.mjs` — download boundary
- `src/*.test.mjs` — pure policy tests

## RULES
Keep bearer tokens out of logs and enforce server-side authorization for every mutation. Test valid and rejected task/artifact identifiers.