import importlib.util
import json
import sys
from pathlib import Path

task_path, workspace = Path(sys.argv[1]), Path(sys.argv[2])
task = json.loads(task_path.read_text(encoding='utf-8'))
result = {'task_id': task['task_id'], 'passed': False, 'error': None}
try:
    source = workspace / 'solution.py'
    if not source.exists():
        raise FileNotFoundError('solution.py missing')
    namespace = {'__name__': 'candidate_solution', '__file__': str(source)}
    exec(compile(source.read_text(encoding='utf-8'), str(source), 'exec'), namespace)
    setup = '\n'.join(task.get('test_imports', []))
    if setup.strip():
        exec(compile(setup, '<mbpp_test_imports>', 'exec'), namespace)
    for index, test in enumerate(task.get('test_list', [])):
        exec(compile(test, f'<mbpp_test_{index}>', 'exec'), namespace)
    result['passed'] = True
except Exception as exc:
    result['error'] = f'{type(exc).__name__}: {exc}'
print(json.dumps(result))
raise SystemExit(0 if result['passed'] else 1)
