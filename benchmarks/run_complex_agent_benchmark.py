#!/usr/bin/env python3
import json, os, shutil, subprocess, tempfile, time
from pathlib import Path

TASKS = [
("stats", """def summarize_numbers(values):
    count=len(values); total=sum(values); mean=total/count
    ordered=sorted(values); median=ordered[len(ordered)//2]
    return {'count':count,'total':total,'mean':mean,'median':median}
""", """import unittest
from solution import summarize_numbers
class Test(unittest.TestCase):
 def test_empty(self): self.assertEqual(summarize_numbers([]), {'count':0,'total':0,'mean':0,'median':0})
 def test_odd(self): self.assertEqual(summarize_numbers([5,1,9]), {'count':3,'total':15,'mean':5.0,'median':5})
 def test_even(self): self.assertEqual(summarize_numbers([1,2,10,20]), {'count':4,'total':33,'mean':8.25,'median':6.0})
 def test_float(self): self.assertEqual(summarize_numbers([1.5,2.5]), {'count':2,'total':4.0,'mean':2.0,'median':2.0})
"""),
("intervals", """def merge_intervals(intervals):
    intervals=sorted(intervals)
    out=[]
    for start,end in intervals:
        if out and start < out[-1][1]: out[-1][1]=max(out[-1][1],end)
        else: out.append([start,end])
    return out
""", """import unittest
from solution import merge_intervals
class Test(unittest.TestCase):
 def test_overlap(self): self.assertEqual(merge_intervals([[1,3],[2,6],[8,10],[9,12]]), [[1,6],[8,12]])
 def test_touching(self): self.assertEqual(merge_intervals([[1,2],[2,3]]), [[1,3]])
 def test_unsorted_and_empty(self): self.assertEqual(merge_intervals([[5,7],[1,2],[-1,0],[]] if False else []), [])
 def test_input_unchanged(self):
  x=[[3,4],[1,2]]; merge_intervals(x); self.assertEqual(x,[[3,4],[1,2]])
"""),
("cache", """class LRUCache:
    def __init__(self, capacity): self.capacity=capacity; self.data={}
    def get(self,key,default=None): return self.data.get(key,default)
    def put(self,key,value):
        self.data[key]=value
        if len(self.data)>self.capacity: self.data.pop(next(iter(self.data)))
""", """import unittest
from solution import LRUCache
class Test(unittest.TestCase):
 def test_eviction_order(self):
  c=LRUCache(2); c.put('a',1); c.put('b',2); self.assertEqual(c.get('a'),1); c.put('c',3); self.assertIsNone(c.get('b')); self.assertEqual(c.get('a'),1)
 def test_update(self):
  c=LRUCache(1); c.put('x',1); c.put('x',2); self.assertEqual(c.get('x'),2)
 def test_default(self): self.assertEqual(LRUCache(1).get('missing',9),9)
"""),
("flatten", """def flatten_config(obj,prefix=''):
    result={}
    for key,value in obj.items():
        name=prefix+'.'+key if prefix else key
        if isinstance(value,dict): result.update(flatten_config(value,name))
        else: result[name]=value
    return result
""", """import unittest
from solution import flatten_config
class Test(unittest.TestCase):
 def test_nested(self): self.assertEqual(flatten_config({'a':{'b':1,'c':{'d':2}},'x':3}), {'a.b':1,'a.c.d':2,'x':3})
 def test_empty(self): self.assertEqual(flatten_config({}),{})
 def test_input(self):
  x={'a':{'b':1}}; flatten_config(x); self.assertEqual(x,{'a':{'b':1}})
"""),
("toposort", """def topological_order(graph):
    result=[]; pending=dict(graph)
    while pending:
        ready=next((n for n,deps in pending.items() if not deps),None)
        if ready is None: return []
        result.append(ready); pending.pop(ready)
        for deps in pending.values(): deps.discard(ready)
    return result
""", """import unittest
from solution import topological_order
class Test(unittest.TestCase):
 def test_order(self):
  r=topological_order({'build':{'compile','test'},'compile':{'generate'},'test':{'compile'},'generate':set()}); self.assertLess(r.index('generate'),r.index('compile')); self.assertLess(r.index('compile'),r.index('test')); self.assertLess(r.index('test'),r.index('build'))
 def test_cycle(self): self.assertEqual(topological_order({'a':{'b'},'b':{'a'}}),[])
 def test_independent(self): self.assertEqual(set(topological_order({'a':set(),'b':set()})),{'a','b'})
"""),
("wordfreq", """import re
def top_words(text,n=3):
    words=re.findall(r'[A-Za-z]+',text.lower()); counts={}
    for w in words: counts[w]=counts.get(w,0)+1
    return sorted(counts.items(),key=lambda p:p[1],reverse=True)[:n]
""", """import unittest
from solution import top_words
class Test(unittest.TestCase):
 def test_counts(self): self.assertEqual(top_words('A b a C b a'), [('a',3),('b',2),('c',1)])
 def test_tie(self): self.assertEqual(top_words('b a'), [('a',1),('b',1)])
 def test_limit(self): self.assertEqual(len(top_words('a b c d',2)),2)
 def test_punctuation(self): self.assertEqual(top_words('Hi, hi!'), [('hi',2)])
"""),
("records", """def dedupe_records(records):
    seen=set(); out=[]
    for record in records:
        key=record.get('id')
        if key not in seen: out.append(record); seen.add(key)
    return out
""", """import unittest
from solution import dedupe_records
class Test(unittest.TestCase):
 def test_keep_last(self): self.assertEqual(dedupe_records([{'id':1,'v':'old'},{'id':2},{'id':1,'v':'new'}]), [{'id':1,'v':'new'},{'id':2}])
 def test_missing_ids(self): self.assertEqual(len(dedupe_records([{'v':1},{'v':2}])),2)
 def test_order(self): self.assertEqual([x['id'] for x in dedupe_records([{'id':3},{'id':1},{'id':3},{'id':2}])],[3,1,2])
"""),
("matrix", """def rotate_matrix(matrix):
    return [list(row) for row in zip(*matrix)]
""", """import unittest
from solution import rotate_matrix
class Test(unittest.TestCase):
 def test_square(self): self.assertEqual(rotate_matrix([[1,2],[3,4]]), [[3,1],[4,2]])
 def test_rect(self): self.assertEqual(rotate_matrix([[1,2,3],[4,5,6]]), [[4,1],[5,2],[6,3]])
 def test_empty(self): self.assertEqual(rotate_matrix([]), [])
 def test_input(self):
  x=[[1,2],[3,4]]; rotate_matrix(x); self.assertEqual(x,[[1,2],[3,4]])
"""),
("paginator", """def pages(items,size):
    return [items[i:i+size] for i in range(0,len(items),size)]
""", """import unittest
from solution import pages
class Test(unittest.TestCase):
 def test_pages(self): self.assertEqual(pages([1,2,3,4,5],2), [[1,2],[3,4],[5]])
 def test_empty(self): self.assertEqual(pages([],2),[])
 def test_bad_size(self):
  with self.assertRaises(ValueError): pages([1],0)
 def test_copy(self):
  x=[1,2]; p=pages(x,1); p[0].append(9); self.assertEqual(x,[1,2])
"""),
("inventory", """def apply_orders(stock,orders):
    result=stock.copy()
    for item,quantity in orders:
        result[item]=result.get(item,0)-quantity
    return result
""", """import unittest
from solution import apply_orders
class Test(unittest.TestCase):
 def test_orders(self): self.assertEqual(apply_orders({'a':5},[('a',2),('b',3)]), {'a':3,'b':0})
 def test_no_mutation(self):
  x={'a':5}; apply_orders(x,[('a',1)]); self.assertEqual(x,{'a':5})
 def test_insufficient(self):
  with self.assertRaises(ValueError): apply_orders({'a':1},[('a',2)])
"""),
("logparser", """def parse_log(lines):
    result=[]
    for line in lines:
        parts=line.strip().split(' '); result.append({'level':parts[0],'message':' '.join(parts[1:])})
    return result
""", """import unittest
from solution import parse_log
class Test(unittest.TestCase):
 def test_basic(self): self.assertEqual(parse_log(['INFO started','ERROR bad thing']),[{'level':'INFO','message':'started'},{'level':'ERROR','message':'bad thing'}])
 def test_blank(self): self.assertEqual(parse_log(['']),[])
 def test_spaces(self): self.assertEqual(parse_log([' WARN   delayed ']),[{'level':'WARN','message':'delayed'}])
"""),
("query", """from urllib.parse import urlparse,parse_qs
def query_params(url):
    return {k:v[0] for k,v in parse_qs(urlparse(url).query).items()}
""", """import unittest
from solution import query_params
class Test(unittest.TestCase):
 def test_basic(self): self.assertEqual(query_params('https://x.test/a?a=1&b=hello%20world'),{'a':'1','b':'hello world'})
 def test_repeated(self): self.assertEqual(query_params('https://x.test?a=1&a=2'),{'a':['1','2']})
 def test_empty(self): self.assertEqual(query_params('https://x.test'),{})
"""),
("brackets", """def balanced(text):
    pairs={')':'(',']':'[','}':'{'}; stack=[]
    for ch in text:
        if ch in '([{': stack.append(ch)
        elif ch in pairs:
            if not stack or stack.pop()!=pairs[ch]: return False
    return not stack
""", """import unittest
from solution import balanced
class Test(unittest.TestCase):
 def test_valid(self): self.assertTrue(balanced('([{}])'))
 def test_invalid(self): self.assertFalse(balanced('([)]')); self.assertFalse(balanced('(')); self.assertFalse(balanced(']'))
 def test_other(self): self.assertTrue(balanced('a + b'))
"""),
("schedule", """def merge_schedule(events):
    events=sorted(events,key=lambda x:x[0]); out=[]
    for start,end in events:
        if out and start <= out[-1][1]: out[-1][1]=max(out[-1][1],end)
        else: out.append([start,end])
    return out
""", """import unittest
from solution import merge_schedule
class Test(unittest.TestCase):
 def test_overlap(self): self.assertEqual(merge_schedule([(1,3),(2,4),(7,8)]),[[1,4],[7,8]])
 def test_touch(self): self.assertEqual(merge_schedule([(1,2),(2,3)]),[[1,3]])
 def test_input(self):
  x=[(3,4),(1,2)]; merge_schedule(x); self.assertEqual(x,[(3,4),(1,2)])
"""),
("rolling", """def rolling_sum(values,window):
    return [sum(values[i:i+window]) for i in range(len(values)-window+1)]
""", """import unittest
from solution import rolling_sum
class Test(unittest.TestCase):
 def test_basic(self): self.assertEqual(rolling_sum([1,2,3,4],2),[3,5,7])
 def test_window_one(self): self.assertEqual(rolling_sum([1,2],1),[1,2])
 def test_bad(self):
  with self.assertRaises(ValueError): rolling_sum([1],0)
 def test_large(self): self.assertEqual(rolling_sum([1,2],5),[])
"""),
("permissions", """def has_permission(user,required):
    return required in user.get('roles',[])
""", """import unittest
from solution import has_permission
class Test(unittest.TestCase):
 def test_direct(self): self.assertTrue(has_permission({'roles':['admin','read']},'read'))
 def test_admin(self): self.assertTrue(has_permission({'roles':['admin']},'delete'))
 def test_missing(self): self.assertFalse(has_permission({},'read'))
 def test_none(self): self.assertFalse(has_permission({'roles':None},'read'))
"""),
("serialize", """import json
def stable_json(data):
    return json.dumps(data)
""", """import unittest
from solution import stable_json
class Test(unittest.TestCase):
 def test_sorted(self): self.assertEqual(stable_json({'b':1,'a':2}), '{"a": 2, "b": 1}')
 def test_nested(self): self.assertEqual(stable_json({'z':[2,1],'a':True}), '{"a": true, "z": [2, 1]}')
"""),
("wordwrap", """def wrap_words(text,width):
    words=text.split(); lines=[]; line=''
    for word in words:
        if len(line)+len(word)+1 <= width: line=(line+' '+word).strip()
        else: lines.append(line); line=word
    if line: lines.append(line)
    return lines
""", """import unittest
from solution import wrap_words
class Test(unittest.TestCase):
 def test_wrap(self): self.assertEqual(wrap_words('one two three',7),['one two','three'])
 def test_exact(self): self.assertEqual(wrap_words('abcd ef',4),['abcd','ef'])
 def test_long(self): self.assertEqual(wrap_words('longword',3),['longword'])
 def test_empty(self): self.assertEqual(wrap_words('',5),[])
"""),
("dependencies", """def dependency_levels(graph):
    levels=[]; remaining={k:set(v) for k,v in graph.items()}
    while remaining:
        ready=[k for k,v in remaining.items() if not v]
        if not ready: return []
        levels.append(ready)
        for k in ready: remaining.pop(k)
        for deps in remaining.values(): deps.difference_update(ready)
    return levels
""", """import unittest
from solution import dependency_levels
class Test(unittest.TestCase):
 def test_levels(self): self.assertEqual(dependency_levels({'a':set(),'b':{'a'},'c':{'a'}}),[['a'],['b','c']])
 def test_cycle(self): self.assertEqual(dependency_levels({'a':{'b'},'b':{'a'}}),[])
 def test_deterministic(self): self.assertEqual(dependency_levels({'b':set(),'a':set()}),[['a','b']])
"""),
("clamp", """def clamp(value,low,high):
    return max(low,min(high,value))
""", """import unittest
from solution import clamp
class Test(unittest.TestCase):
 def test_inside(self): self.assertEqual(clamp(5,0,10),5)
 def test_edges(self): self.assertEqual(clamp(-1,0,10),0); self.assertEqual(clamp(11,0,10),10)
 def test_bad_range(self):
  with self.assertRaises(ValueError): clamp(1,10,0)
"""),
]

def main():
    root=Path(tempfile.mkdtemp(prefix='ozymandias-complex-bench-'))
    results=[]
    try:
        binary=Path(__file__).parents[1]/'target/release/ozymandias'
        model=os.environ.get('OZY_MODEL','gpt-5-mini')
        for index,(name,source,tests) in enumerate(TASKS,1):
            taskdir=root/f'{index:02d}_{name}'; taskdir.mkdir()
            (taskdir/'solution.py').write_text(source)
            (taskdir/'test_solution.py').write_text(tests)
            (taskdir/'README.md').write_text(f'# {name}\nRepair solution.py without modifying test_solution.py.\n')
            objective=(f'Debug this existing multi-step task. Inspect solution.py, test_solution.py, and README.md. '
                       f'Reproduce failures, diagnose every defect, modify only solution.py, preserve its public API, '
                       f'and run the complete test suite before finishing. Do not change the tests.')
            started=time.time()
            proc=subprocess.run([str(binary),objective],cwd=taskdir,env={**os.environ,'OZY_MODEL':model},text=True,capture_output=True,timeout=150)
            score=subprocess.run(['python3','-m','py_compile','solution.py','test_solution.py'],cwd=taskdir,text=True,capture_output=True)
            test=subprocess.run(['python3','-m','unittest','discover','-s','.','-p','test_solution.py'],cwd=taskdir,text=True,capture_output=True)
            passed=score.returncode==0 and test.returncode==0
            results.append({'id':index,'name':name,'passed':passed,'agent_returncode':proc.returncode,'steps':json.loads(proc.stdout).get('steps') if proc.stdout.strip().startswith('{') else None,'duration_s':round(time.time()-started,2),'stdout_tail':proc.stdout[-1200:],'stderr_tail':proc.stderr[-800:]})
            print(f'{index:02d}/{len(TASKS)} {name}: {"PASS" if passed else "FAIL"}',flush=True)
        summary={'benchmark':'Complex Agent Tool/Debug Benchmark','tasks':len(TASKS),'passed':sum(r['passed'] for r in results),'pass_rate':sum(r['passed'] for r in results)/len(results),'results':results}
        out=Path('/home/ubuntu/ozymandias/benchmarks/results/complex_agent_benchmark.json'); out.write_text(json.dumps(summary,indent=2))
        print(json.dumps({'tasks':summary['tasks'],'passed':summary['passed'],'pass_rate':summary['pass_rate'],'result_path':str(out)}))
    finally:
        shutil.rmtree(root,ignore_errors=True)

if __name__=='__main__': main()
