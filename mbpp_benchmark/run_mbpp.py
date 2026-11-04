import json
import os
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path('/home/ubuntu/ozymandias')
DATA = ROOT / 'mbpp_benchmark' / 'data' / 'sanitized-mbpp.json'
BINARY = ROOT / 'target' / 'release' / 'ozymandias'
EVAL = ROOT / 'mbpp_benchmark' / 'evaluate_mbpp.py'
RUN_ROOT = ROOT / 'mbpp_benchmark' / 'runs'


def load_tasks():
    data = json.loads(DATA.read_text(encoding='utf-8'))
    return sorted(data, key=lambda x: int(x['task_id']))


def run_task(task, run_dir, timeout=240):
    task_id = int(task['task_id'])
    work = run_dir / f'task_{task_id:04d}'
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True, exist_ok=True)
    tests = '\n'.join(task.get('test_list', []))
    imports = '\n'.join(task.get('test_imports', []))
    prompt = (
        'Solve this sanitized MBPP programming problem. Create solution.py containing the complete Python solution. '
        'Preserve any imports or helper definitions needed by the prompt, and make the requested function available at module scope. '
        'Create test_solution.py with targeted tests and README.md. Run verification with the supplied assertions. '
        'Do not modify the agent source. Finish only after checking the solution.\n\n'
        f'Task ID: {task_id}\nProblem: {task["prompt"]}\n'
        f'Test imports/setup:\n{imports}\nTests:\n{tests}'
    )
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
    task_file = work / 'task.json'
    task_file.write_text(json.dumps(task), encoding='utf-8')
    eval_proc = subprocess.run([sys.executable, str(EVAL), str(task_file), str(work)], capture_output=True, text=True, timeout=30)
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
        'task_id': task_id,
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
    run_id = time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())
    run_dir = RUN_ROOT / run_id
    run_dir.mkdir(parents=True, exist_ok=False)
    tasks = load_tasks()
    results = []
    print(f'START MBPP sanitized run {run_id}: {len(tasks)} tasks', flush=True)
    with ThreadPoolExecutor(max_workers=1) as pool:
        futures = [pool.submit(run_task, task, run_dir) for task in tasks]
        for i, future in enumerate(as_completed(futures), 1):
            result = future.result()
            results.append(result)
            if i % 10 == 0 or i == len(tasks):
                passed = sum(r['passed'] for r in results)
                print(f'PROGRESS {i}/{len(tasks)} passed={passed}', flush=True)
    results.sort(key=lambda r: r['task_id'])
    report = {
        'benchmark': 'MBPP sanitized',
        'dataset_path': str(DATA),
        'task_count': len(results),
        'passed': sum(bool(r['passed']) for r in results),
        'artifact_passed': sum(bool(r['artifact_passed']) for r in results),
        'timed_out': sum(bool(r['timed_out']) for r in results),
        'protocol_error_tasks': sum(bool(r['protocol_errors']) for r in results),
        'total_protocol_errors': sum(r.get('protocol_errors', 0) for r in results),
        'total_tool_actions': sum(r.get('tool_actions', 0) for r in results),
        'run_id': run_id,
        'results': results,
    }
    report['pass_rate'] = report['passed'] / report['task_count'] if report['task_count'] else 0.0
    (run_dir / 'summary.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    (RUN_ROOT / 'latest_summary.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({k: report[k] for k in report if k != 'results'}, indent=2), flush=True)


if __name__ == '__main__':
    main()
