# Telegrambot — Progress Log

## Phase 1: Core Bot ✅

### Completed (2026-03-08/09)

- [x] **Crate setup**: Created `bins/telegrambot` with `Cargo.toml`, added to workspace
- [x] **Core implementation**: `main.rs` with config, data fetching, indicators,
      Browserless screenshots, LLM agent loop, and Telegram messaging
- [x] **Module refactoring**: Split 1059-line monolith into 8 focused files
      across 6 modules (config, data, browserless, chart, llm, telegram)
- [x] **Chart template**: Extracted to `chart_template.html` loaded via `include_str!`
- [x] **Test harness**: `src/bin/test_analysis.rs` — runs analysis without Telegram
- [x] **K8s manifests**: Browserless + LiteLLM deployment yamls
- [x] **E2E test**: Successful BTC-USDT analysis via gpt-4o (GitHub Copilot)

### Phase 1.1: Output Quality & Config Extraction (2026-03-09)

- [x] **Concise system prompt**: Rewritten with ≤300 word limit, mandatory chart
      capture, no follow-up questions, ticker in title
- [x] **Unicode ticker header**: Programmatic `━━━━━━ 🔔 𝗕𝗧𝗖-𝗨𝗦𝗗𝗧 ━━━━━━` +
      timestamp, injected by code in `telegram.rs` (not LLM-dependent)
- [x] **All-TF chart album**: Charts captured for every configured timeframe
      in `main.rs`, sent as Telegram media group (photo album)
- [x] **External prompt config**: System prompt, user message, and tool schemas
      extracted to `config/prompts/{system.txt, user.txt, tools.json}`
- [x] **Runtime prompt loading**: `llm/mod.rs` loads from `PROMPTS_DIR` with
      `{{placeholder}}` substitution — no recompile for prompt changes
- [x] **K8s ConfigMap**: `k8s/prompts-configmap.yaml` + volume mount in
      `k8s/telegrambot.yaml` — prompt changes via `kubectl apply`
- [x] **Debian Dockerfile**: Switched from Alpine/musl to `rust:slim-bookworm` /
      `debian:bookworm-slim` — uses system OpenSSL, much faster builds
- [x] **Dockerfile prompt fallback**: Default prompts COPY'd into image, overridden
      by ConfigMap mount in K8s
- [x] **K8s deployment**: Successfully deployed and ran first analysis cycle
      in local K8s cluster

### Key Decisions

1. **`async-openai` 0.33**: Requires `chat-completion` feature flag. Types live in
   `async_openai::types::chat::*`. Tool calls use `ChatCompletionTools::Function()`
   enum variant, not a builder.

2. **LiteLLM stateless mode**: Uses inline `master_key` in `config.yaml` +
   `allow_requests_on_db_unavailable: true` + `disable_spend_logs: true`.
   No database needed for pure proxy usage.

3. **GitHub Copilot OAuth**: The `github_copilot/` provider prefix triggers OAuth
   device flow on startup. Pod probes need generous initial delays (120s liveness)
   to survive the auth wait.

4. **Browserless health**: Does not serve `/` — use TCP socket probes instead
   of HTTP health checks.

5. **Crate structure**: `lib.rs` re-exports modules so both `main.rs` and
   `bin/test_analysis.rs` can share the same code.

6. **External prompts**: System prompt, user message, and tool schemas are loaded
   from files at runtime (`PROMPTS_DIR`) with mustache-style `{{placeholder}}`
   rendering. This avoids recompilation for prompt tweaks.

7. **Debian over Alpine**: Alpine/musl builds required OpenSSL source compilation
   (~45 min). Debian uses system `libssl-dev` — same build in ~20 min, ~60MB
   larger runtime image (acceptable trade-off).

### E2E Test Results (2026-03-09 01:08 ICT)

```text
Symbol:     BTC-USDT
Model:      gpt-4o via litellm (GitHub Copilot)
Timeframes: 1m, 5m, 15m, 1h, 4h, 1d, 1w, 1M (8 total)
Candles:    1440 per TF (70 for 1M, 301 for 1w)

Agent turns: 4
  Turn 0: get_multi_tf_overview
  Turn 1: get_indicator_summary ×3 (1h, 4h, 1d)
  Turn 2: capture_chart (1h)
  Turn 3: capture_chart (4h, 1d) + final analysis

Result: Structured analysis with market structure, momentum,
        key levels, risk assessment, and recommendation.
        Chart screenshots captured via Browserless.
```

---

## Phase 2: Disposable Python Indicators 🔲

**Status**: Not started. Planned for a separate iteration.

See `architecture.md` for the PyO3 design.

---

## Known Issues / TODOs

- `timeout_secs` field in `EnvConf` is unused (harmless warning)
- Chart template uses CDN-loaded JS (unpkg, jsdelivr) — consider bundling
  for air-gapped deployments
- LiteLLM GitHub Copilot auth token is ephemeral (lost on pod restart) —
  consider a PersistentVolume for `/root/.config/litellm/` in production
- No retry logic on LLM API failures yet
- Docker build still recompiles all deps on `bins/` changes — consider
  cargo-chef for dependency layer caching
- All TF charts currently use the same HTML (default TF view) — per-TF
  chart rendering could be improved to switch the active TF
