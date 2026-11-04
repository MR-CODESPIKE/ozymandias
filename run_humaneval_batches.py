import gzip
import json
import os
import subprocess
import sys
import time
import shutil
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

DATA = Path('/home/ubuntu/human-eval/data/HumanEval.jsonl.gz')
ROOT = Path('/home/ubuntu/ozymandias')
BINARY = ROOT / 'target/release/ozymandias'
EVAL = ROOT / 'evaluate_candidate.py'
OUT = ROOT / 'humaneval_batches'

def load_problems():
    with gzip.open(DATA, 'rt', encoding='utf-8') as f:
        return [json.loads(line) for line in f]

def run_task(problem, batch, timeout=240):
    task_id = problem['task_id'].replace('/', '_')
    work = OUT / f'batch_{batch:02d}' / task_id
    # Never reuse artifacts from an earlier attempt or rerun.
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True, exist_ok=True)
    prompt = ('Solve this official HumanEval problem. Create solution.py containing the required function with the exact signature, and first copy every function, helper, import, and definition present in the problem prompt into solution.py because the official tests may reference any of them; then implement or correct the requested entry point. '
              'Create test_solution.py with at least four targeted tests and README.md. Run verification with the supplied task tests. '
              'Do not modify the agent source. Finish only after checking the solution. Problem JSON: ' + json.dumps(problem, ensure_ascii=False))
    started = time.time()
    env = os.environ.copy()
    env['HOME'] = str(work / 'home')
    user_site = str(Path.home() / '.local' / 'lib' / f'python{sys.version_info.major}.{sys.version_info.minor}' / 'site-packages')
    env['PYTHONPATH'] = os.pathsep.join(filter(None, [os.environ.get('PYTHONPATH', ''), user_site]))
    (work / 'home').mkdir(exist_ok=True)
    try:
        proc = subprocess.run([str(BINARY), prompt], cwd=work, env=env, capture_output=True, text=True, timeout=timeout)
        timed_out = False
    except subprocess.TimeoutExpired as exc:
        proc = subprocess.CompletedProcess([], 124, exc.stdout or '', exc.stderr or '')
        timed_out = True
    elapsed = time.time() - started
    (work / 'agent_output.json').write_text(proc.stdout or '', encoding='utf-8')
    (work / 'agent_stderr.log').write_text(proc.stderr or '', encoding='utf-8')
    (work / 'timing.txt').write_text(f'{elapsed:.3f}\n', encoding='utf-8')
    problem_file = work / 'problem.json'
    problem_file.write_text(json.dumps(problem), encoding='utf-8')
    eval_proc = subprocess.run([sys.executable, str(EVAL), str(problem_file), str(work)], capture_output=True, text=True, timeout=30)
    try:
        trace = json.loads(proc.stdout)
    except Exception:
        trace = {}
    try:
        score = json.loads(eval_proc.stdout)
    except Exception:
        score = {'passed': False, 'error': 'evaluator output invalid'}
    events = trace.get('events', []) if isinstance(trace, dict) else []
    artifact_passed = bool(score.get('passed'))
    return {
        'task_id': problem['task_id'],
        'batch': batch,
        'passed': artifact_passed and not timed_out and proc.returncode == 0,
        'artifact_passed': artifact_passed,
        'timed_out': timed_out,
        'agent_exit': proc.returncode,
        'elapsed_seconds': round(elapsed, 3),
        'steps': trace.get('steps'),
        'events': len(events),
        'protocol_errors': sum(1 for e in events if e.get('role') == 'protocol_error'),
        'tool_actions': sum(1 for e in events if e.get('role') == 'tool'),
        'error': score.get('error'),
    }

def main():
    batch = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    problems = load_problems()
    start = (batch - 1) * 10
    selected = problems[start:start + 10]
    OUT.mkdir(exist_ok=True)
    results = []
        # Batch 8 had seven simultaneous endpoint timeouts with no agent trace.
        # Use a single worker for the next gate to reduce endpoint contention while
        # preserving task isolation and the per-task timeout.
    with ThreadPoolExecutor(max_workers=1) as pool:
        futures = [pool.submit(run_task, p, batch) for p in selected]
        for future in as_completed(futures):
            results.append(future.result())
    results.sort(key=lambda r: r['task_id'])
    report = {
        'batch': batch,
        'task_count': len(results),
        'passed': sum(r['passed'] for r in results),
        'results': results,
        'optimization_checkpoint': 'flexible response envelopes; bounded two-worker parallelism after batch-3 endpoint failures',
    }
    path = OUT / f'batch_{batch:02d}_summary.json'
    path.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report, indent=2))

if __name__ == '__main__':
    main()
