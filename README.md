# Ozymandias

Ozymandias is a Rust-native autonomous coding agent with an OpenAI-compatible model adapter, safe shell/file tools, bounded memory, action-envelope normalization, local verification, and fatal provider-error handling.

## Build and test

```bash
cargo fmt --check
cargo test
cargo build --release
```

The release binary is `target/release/ozymandias`. Configure the model endpoint and credentials in the process environment only:

```bash
export OPENAI_API_BASE="https://example.invalid/v1"
export OPENAI_API_KEY="..."
export OZY_MODEL="gpt-5-nano"
```

Credentials are intentionally not stored in this repository.

## Benchmark harnesses

The repository includes runners for HumanEval and sanitized MBPP. Generated datasets, task workspaces, logs, benchmark summaries, and build outputs are excluded by `.gitignore` so benchmark artifacts remain local.

- `run_full_humaneval.py` runs the HumanEval suite.
- `mbpp_benchmark/run_mbpp.py` runs sanitized MBPP when its dataset is present at `mbpp_benchmark/data/sanitized-mbpp.json`.
- `mbpp_benchmark/evaluate_mbpp.py` evaluates generated MBPP artifacts.

## Current validation

The current optimized source passes all 8 Rust unit tests and builds successfully in release mode. Recent local benchmark work focused on flexible model response envelopes and repair of double-escaped Python source returned by some providers.
