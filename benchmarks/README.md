# Ozymandias benchmark results

These are local benchmark measurements of the Rust-native Ozymandias coding agent using the `gpt-5-nano` model through an OpenAI-compatible proxy. Each task ran in a fresh workspace and was scored by executing the generated artifact against the benchmark tests.

## Results

| Benchmark | Tasks completed with usable quota | Passed | Rate | Full-suite accounting |
|---|---:|---:|---:|---:|
| HumanEval | 164 | 112 | 68.29% | 112/164, no timeouts |
| Sanitized MBPP, initial run | 120 | 92 | 76.67% | 92/427 = 21.55% after quota exhaustion |
| Sanitized MBPP, parser-fix rerun | 240 | 195 | 81.25% | 195/427 = 45.67% after quota exhaustion |
| Sanitized MBPP, post-fix canary | 10 | 6 | 60.00% | Focused regression sample |
| Complex Agent Tool/Debug Benchmark | 20 | 20 | 100.00% | Local 20-task end-to-end tool benchmark |
| BrowseComp pilot, seed 0 | 20 | 18 | Not reported | 2 judge calls failed from provider quota; 0/18 correct |

The MBPP full-suite totals are quota-contaminated: the provider stopped supplying usable responses after approximately task 120 in the initial run and approximately task 240 in the rerun. No MBPP task timed out in either full run. The quota-adjusted rates above use only the tasks reached before that boundary and should not be presented as complete 427-task scores.

BrowseComp uses OpenAI's official encrypted 1,266-question test file and the published short-answer grading format. The 20-question pilot used the official random seed 0 sample. The first attempt reached 20/20 questions and scored 1/20 (5%), but the later clean retry reached only 18/20 because two judge calls returned empty provider responses. Per the runner's completeness gate, the clean retry is marked incomplete and has no aggregate accuracy. The encrypted question file and raw BrowseComp outputs are intentionally excluded from the public repository.

## Optimization evidence

The rerun used two reliability improvements. The action parser accepts nested provider envelopes such as `{"write_bundle": ...}` and `{"action":{"type":"write_bundle", ...}}`. The write boundary also repairs double-escaped Python or Rust source when a provider returns a one-line file containing literal `\\n` sequences. The parser and source-repair changes are covered by Rust unit tests.

Raw JSON summaries are stored in [`results/`](results/). The benchmark runners remain in [`../run_full_humaneval.py`](../run_full_humaneval.py), [`../mbpp_benchmark/run_mbpp.py`](../mbpp_benchmark/run_mbpp.py), and [`browsecomp/run_browsecomp_subset.py`](browsecomp/run_browsecomp_subset.py).
