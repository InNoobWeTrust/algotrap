# Changelog: Bot Deploy Path and Dataframe Migration

**Requirement**: `docs/trds/dataframe-boundary-refactor.md`, `docs/prds/research/dataframe-engine-build-strategy.md`
**Status**: completed

---

## Session: 2026-04-05T22:55

### Summary

Stabilized the telegrambot and cryptobot Docker/CI deployment path for local and CI builds. Added the first research and TRD artifacts for decoupling dataframe consumers from previous dataframe implementation-specific engine APIs.

### Changes

- Added: `docs/prds/research/dataframe-engine-build-strategy.md` and `docs/trds/dataframe-boundary-refactor.md` — document the migration path toward an engine boundary and possible DuckDB evaluation.
- Added: `.dockerignore` and `bins/cryptobot/.dockerignore` — align ignored files with the actual Docker build contexts.
- Modified: `bins/telegrambot/src/commands.rs` — downgrade the expected Telegram polling conflict to warning level while preserving error logging for other listener failures.
- Modified: telegrambot and cryptobot Docker/CI/build-context files — enable cache-aware native rebuilds locally while keeping the CI target wiring explicit.

### Decisions

- Keep previous dataframe implementation as the current implementation and document an engine-boundary refactor before any DuckDB migration attempt.
- Keep one shared Dockerfile path per service and express platform differences in CI/config rather than by forking Dockerfiles.

### Verification Status

- `cargo check --offline -p telegrambot`: passed
- `docker build` planner smoke checks for telegrambot and cryptobot: passed
- `kubectl rollout` for telegrambot and cryptobot: passed
