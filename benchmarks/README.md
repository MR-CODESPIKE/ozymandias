# Ozymandias benchmark results

These are local benchmark measurements of the Rust-native Ozymandias coding agent using the `gpt-5-nano` model through an OpenAI-compatible proxy. Each task ran in a fresh workspace and was scored by executing the generated artifact against the benchmark tests.

## Results

| Benchmark | Tasks completed with usable quota | Passed | Rate | Full-suite accounting |
|---|---:|---:|---:|---:|
| HumanEval | 164 | 112 | 68.29% | 112/164, no timeouts |
| Sanitized MBPP, initial run | 120 | 92 | 76.67% | 92/427 = 21.55% after quota exhaustion |
| Sanitized MBPP, parser-fix rerun | 240 | 195 | 81.25% | 195/427 = 45.67% after quota exhaustion |
| Sanitized MBPP, post-fix canary | 10 | 6 | 60.00% | Focused regression sample |

The MBPP full-suite totals are quota-contaminated: the provider stopped supplying usable responses after approximately task 120 in the initial run and approximately task 240 in the rerun. No MBPP task timed out in either full run. The quota-adjusted rates above use only the tasks reached before that boundary and should not be presented as complete 427-task scores.

## Optimization evidence

The rerun used two reliability improvements. The action parser accepts nested provider envelopes such as `{"write_bundle": ...}` and `{"action":{"type":"write_bundle", ...}}`. The write boundary also repairs double-escaped Python or Rust source when a provider returns a one-line file containing literal `\\n` sequences. The parser and source-repair changes are covered by Rust unit tests.

Raw JSON summaries are stored in [`results/`](results/). The benchmark runners remain in [`../run_full_humaneval.py`](../run_full_humaneval.py) and [`../mbpp_benchmark/run_mbpp.py`](../mbpp_benchmark/run_mbpp.py).
