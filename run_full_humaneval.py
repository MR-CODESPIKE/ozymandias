import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path('/home/ubuntu/ozymandias')
OUT = ROOT / 'humaneval_batches'
LOG = OUT / 'full_suite_logs'
TOTAL_BATCHES = 17


def main():
    OUT.mkdir(exist_ok=True)
    LOG.mkdir(exist_ok=True)
    summaries = []
    for batch in range(1, TOTAL_BATCHES + 1):
        log_path = LOG / f'batch_{batch:02d}.log'
        print(f'START batch {batch}/{TOTAL_BATCHES}', flush=True)
        with log_path.open('w', encoding='utf-8') as log:
            proc = subprocess.run(
                [sys.executable, str(ROOT / 'run_humaneval_batches.py'), str(batch)],
                cwd=ROOT,
                env=os.environ.copy(),
                stdout=log,
                stderr=subprocess.STDOUT,
                text=True,
            )
        summary_path = OUT / f'batch_{batch:02d}_summary.json'
        if summary_path.exists():
            summary = json.loads(summary_path.read_text(encoding='utf-8'))
        else:
            summary = {
                'batch': batch,
                'task_count': 0,
                'passed': 0,
                'results': [],
                'runner_exit': proc.returncode,
                'error': 'summary missing',
            }
        summaries.append(summary)
        print(
            f'END batch {batch}: {summary.get("passed", 0)}/{summary.get("task_count", 0)} '
            f'(runner_exit={proc.returncode})',
            flush=True,
        )

    results = [r for s in summaries for r in s.get('results', [])]
    report = {
        'problem_count': len(results),
        'passed': sum(bool(r.get('passed')) for r in results),
        'artifact_passed': sum(bool(r.get('artifact_passed')) for r in results),
        'timed_out': sum(bool(r.get('timed_out')) for r in results),
        'protocol_error_tasks': sum(bool(r.get('protocol_errors')) for r in results),
        'total_protocol_errors': sum(r.get('protocol_errors', 0) for r in results),
        'total_tool_actions': sum(r.get('tool_actions', 0) for r in results),
        'batches': summaries,
    }
    report['pass_rate'] = (report['passed'] / report['problem_count']) if report['problem_count'] else 0.0
    path = OUT / 'full_suite_summary.json'
    path.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps({k: report[k] for k in report if k != 'batches'}, indent=2), flush=True)


if __name__ == '__main__':
    main()
