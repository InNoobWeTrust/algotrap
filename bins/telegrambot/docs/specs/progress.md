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

### E2E Test Results (2026-03-09 01:08 ICT)

```
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
- Telegram message formatting could use Markdown/HTML parse mode
