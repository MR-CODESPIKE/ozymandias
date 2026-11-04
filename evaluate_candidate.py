import json, sys, importlib.util
from pathlib import Path

problem_path, workspace = Path(sys.argv[1]), Path(sys.argv[2])
p = json.loads(problem_path.read_text())
result = {'task_id': p['task_id'], 'passed': False, 'error': None}
try:
    source = workspace / 'solution.py'
    if not source.exists(): raise FileNotFoundError('solution.py missing')
    spec = importlib.util.spec_from_file_location('candidate_solution', source)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    namespace = {}
    exec(p['test'], namespace)
    candidate = getattr(mod, p['entry_point'])
    namespace['check'](candidate)
    result['passed'] = True
except Exception as exc:
    result['error'] = f'{type(exc).__name__}: {exc}'
print(json.dumps(result))
raise SystemExit(0 if result['passed'] else 1)
