# FRONTEND KNOWLEDGE

## OVERVIEW
The frontend tree contains a root UI plus independent master and worker Vite applications.

## WHERE TO LOOK
- `master-ui/src/App.jsx` — task/admin workflow
- `master-ui/src/*Policy.mjs` — pure policy helpers/tests
- `worker-ui/src/App.jsx` — worker registration/status workflow
- `worker-ui/src/workerProfile.mjs` — registration contract/tests

## CONVENTIONS
- Keep surfaces visually consistent, keyboard accessible, and API contracts explicit.
- Backend auth/ownership is authoritative; browser checks are convenience only.
- Keep policy logic pure and covered by Node tests.
- Use each app's own lockfile for reproducible builds.

## ANTI-PATTERNS
- No secrets or authorization decisions in client bundles.
- No wildcard origins or unsafe task/artifact identifiers.
- Do not call a build green when tests were skipped.