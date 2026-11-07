#!/usr/bin/env python3
import base64, csv, hashlib, json, os, random, re, shutil, subprocess, tempfile, time
from pathlib import Path
from openai import OpenAI

QUERY_TEMPLATE = """{question}

You are evaluating a browsing agent. Use available web tools and reason carefully across sources. Return your final response in exactly this format:
Explanation: {{your concise evidence-based explanation}}
Exact Answer: {{your succinct final answer}}
Confidence: {{your confidence score between 0% and 100%}}"""

GRADER_TEMPLATE = """Judge whether the following response to the question is correct based on the correct answer.

Question: {question}
Response: {response}
Correct answer: {correct_answer}

Return exactly one line: CORRECT: yes or CORRECT: no.
Treat equivalent wording and small numerical formatting differences as correct, but do not award credit for ambiguity or a materially different answer."""

def decrypt(ciphertext_b64, password):
    encrypted = base64.b64decode(ciphertext_b64)
    key = hashlib.sha256(password.encode()).digest()
    key = (key * (len(encrypted) // len(key) + 1))[:len(encrypted)]
    return bytes(a ^ b for a, b in zip(encrypted, key)).decode()

def judge(client, model, question, answer, response):
    r = client.chat.completions.create(
        model=model,
        messages=[{"role":"user", "content":GRADER_TEMPLATE.format(question=question, correct_answer=answer, response=response)}],
        max_completion_tokens=120,
    )
    text = r.choices[0].message.content or ""
    if not text:
        raise RuntimeError("judge returned empty content; provider quota or response failure")
    return bool(re.search(r"CORRECT:\s*yes", text, re.I)), text

def main():
    root = Path(__file__).parents[1]
    data = root / "browsecomp/data/browse_comp_test_set.csv"
    binary = root.parent / "target/release/ozymandias"
    n = int(os.environ.get("BROWSECOMP_N", "20"))
    agent_model = os.environ.get("OZY_MODEL", "gpt-5-mini")
    judge_model = os.environ.get("BROWSECOMP_JUDGE_MODEL", agent_model)
    with data.open(newline="") as f:
        rows = list(csv.DictReader(f))
    selected = random.Random(0).sample(rows, n)
    client = OpenAI()
    results = []
    work_root = Path(tempfile.mkdtemp(prefix="ozymandias-browsecomp-"))
    try:
        for i, row in enumerate(selected, 1):
            question = decrypt(row["problem"], row["canary"])
            correct = decrypt(row["answer"], row["canary"])
            taskdir = work_root / f"task_{i:03d}"; taskdir.mkdir()
            prompt = QUERY_TEMPLATE.format(question=question)
            started = time.time()
            try:
                proc = subprocess.run([str(binary), prompt], cwd=taskdir, env={**os.environ, "OZY_MODEL":agent_model}, text=True, capture_output=True, timeout=240)
                raw = proc.stdout.strip()
                try:
                    payload = json.loads(raw)
                    response = payload.get("answer", raw)
                    steps = payload.get("steps")
                    events = payload.get("events", [])
                except Exception:
                    response, steps, events = raw, None, []
                verdict, judge_text = judge(client, judge_model, question, correct, response)
                status = "PASS" if verdict else "FAIL"
                result = {"index":i,"passed":verdict,"agent_returncode":proc.returncode,"steps":steps,"duration_s":round(time.time()-started,2),"response":response,"judge":judge_text,"tool_events":sum(e.get("role")=="tool" for e in events if isinstance(e,dict))}
            except Exception as exc:
                status = "ERROR"
                result = {"index":i,"passed":False,"error":repr(exc),"duration_s":round(time.time()-started,2)}
            results.append(result)
            print(f"{i:02d}/{n} {status}", flush=True)
        out = root / "results" / f"browsecomp_subset_{n}.json"; out.parent.mkdir(exist_ok=True)
        completed = sum("error" not in x for x in results)
        passed = sum(x.get("passed", False) for x in results)
        summary = {"benchmark":"BrowseComp official encrypted test set","sample_size":n,"seed":0,"agent_model":agent_model,"judge_model":judge_model,"completed":completed,"passed":passed,"status":"complete" if completed == n else "incomplete","accuracy":passed/n if completed == n else None,"results":results,"reference":"OpenAI simple-evals browsecomp_eval.py"}
        out.write_text(json.dumps(summary, indent=2))
        print(json.dumps({k:summary[k] for k in ["sample_size","passed","accuracy","agent_model","judge_model","reference"]}))
    finally:
        shutil.rmtree(work_root, ignore_errors=True)

if __name__ == "__main__": main()
