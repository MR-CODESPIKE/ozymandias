import gzip
import json
from pathlib import Path
from run_humaneval_batches import run_task

DATA = Path('/home/ubuntu/human-eval/data/HumanEval.jsonl.gz')
with gzip.open(DATA, 'rt', encoding='utf-8') as handle:
    problem = json.loads(handle.readline())
result = run_task(problem, batch=18, timeout=240)
print(json.dumps(result, indent=2))
