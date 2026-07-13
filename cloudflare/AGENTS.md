# CLOUDFLARE SERVICE

## OVERVIEW
Separate update/artifact administration surface; not the Rust task runtime.

## RULES
Treat cookies, admin routes, KV/R2 metadata, and update artifacts as security-sensitive. Validate origin/CSRF assumptions, rate-limit login, and do not expose operational metadata unnecessarily. Keep this release contract separate from Hivemind binaries.