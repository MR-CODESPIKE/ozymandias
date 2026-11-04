use crate::sandbox::Sandbox;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub passed: bool,
    pub checks: Vec<CheckResult>,
}

impl VerificationResult {
    pub fn detail(&self) -> String {
        self.checks
            .iter()
            .map(|check| {
                format!(
                    "{}: {}\n{}",
                    if check.passed { "PASS" } else { "FAIL" },
                    check.name,
                    check.detail
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub async fn verify_workspace(sandbox: &Sandbox) -> Result<VerificationResult> {
    let root = &sandbox.root;
    let mut checks = Vec::new();

    let solution = root.join("solution.py");
    let tests = root.join("test_solution.py");
    if solution.exists() {
        checks.push(
            run_check(
                sandbox,
                "python syntax",
                "python3",
                &["-m", "py_compile", "solution.py"],
            )
            .await,
        );
        if tests.exists() {
            checks.push(run_python_tests(sandbox).await);
        }
    }

    if root.join("Cargo.toml").exists() {
        checks.push(run_check(sandbox, "rust tests", "cargo", &["test"]).await);
    }

    let package = root.join("package.json");
    if package.exists() {
        checks.push(
            run_check(
                sandbox,
                "javascript package tests",
                "npm",
                &["test", "--", "--runInBand"],
            )
            .await,
        );
    }
    for path in javascript_files(root) {
        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let name = format!("javascript syntax: {display}");
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        checks.push(run_check(sandbox, &name, "node", &["--check", &relative]).await);
    }

    if checks.is_empty() {
        checks.push(CheckResult {
            name: "artifact presence".into(),
            passed: false,
            detail: "no recognized artifact or project manifest found".into(),
        });
    }

    Ok(VerificationResult {
        passed: checks.iter().all(|check| check.passed),
        checks,
    })
}

async fn run_check(sandbox: &Sandbox, name: &str, program: &str, args: &[&str]) -> CheckResult {
    match sandbox.exec(program, args).await {
        Ok(output) if output.status == 0 => CheckResult {
            name: name.into(),
            passed: true,
            detail: truncate(format!("{}{}", output.stdout, output.stderr)),
        },
        Ok(output) => CheckResult {
            name: name.into(),
            passed: false,
            detail: truncate(format!(
                "exit status {}\nstdout:\n{}\nstderr:\n{}",
                output.status, output.stdout, output.stderr
            )),
        },
        Err(error) => CheckResult {
            name: name.into(),
            passed: false,
            detail: error.to_string(),
        },
    }
}

fn truncate(mut text: String) -> String {
    const LIMIT: usize = 4000;
    if text.len() > LIMIT {
        text.truncate(LIMIT);
        text.push_str("…[truncated]");
    }
    text
}

async fn run_python_tests(sandbox: &Sandbox) -> CheckResult {
    let pytest = sandbox
        .exec("python3", &["-m", "pytest", "-q", "test_solution.py"])
        .await;
    match pytest {
        Ok(output) if output.status == 0 => CheckResult {
            name: "python tests (pytest)".into(),
            passed: true,
            detail: truncate(format!("{}{}", output.stdout, output.stderr)),
        },
        Ok(output)
            if output.stderr.contains("No module named pytest")
                || output.stdout.contains("No module named pytest") =>
        {
            match sandbox
                .exec(
                    "python3",
                    &[
                        "-m",
                        "unittest",
                        "discover",
                        "-s",
                        ".",
                        "-p",
                        "test_solution.py",
                    ],
                )
                .await
            {
                Ok(fallback) => CheckResult {
                    name: "python tests (unittest fallback)".into(),
                    passed: fallback.status == 0,
                    detail: truncate(format!(
                        "exit status {}\nstdout:\n{}\nstderr:\n{}",
                        fallback.status, fallback.stdout, fallback.stderr
                    )),
                },
                Err(error) => CheckResult {
                    name: "python tests (unittest fallback)".into(),
                    passed: false,
                    detail: error.to_string(),
                },
            }
        }
        Ok(output) => CheckResult {
            name: "python tests (pytest)".into(),
            passed: false,
            detail: truncate(format!(
                "exit status {}\nstdout:\n{}\nstderr:\n{}",
                output.status, output.stdout, output.stderr
            )),
        },
        Err(error) => CheckResult {
            name: "python tests (pytest)".into(),
            passed: false,
            detail: error.to_string(),
        },
    }
}

fn javascript_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("js") {
            files.push(path);
        }
    }
    files
}

pub fn evaluate_artifact(path: impl AsRef<Path>) -> Result<Evaluation> {
    let path = path.as_ref();
    Ok(Evaluation {
        passed: path.exists(),
        detail: if path.exists() {
            "artifact exists".into()
        } else {
            "artifact missing".into()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::verify_workspace;
    use crate::sandbox::Sandbox;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn verifies_python_syntax_and_tests() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("solution.py"),
            "def answer():\n    return 42\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("test_solution.py"),
            "import unittest\nfrom solution import answer\n\nclass TestSolution(unittest.TestCase):\n    def test_answer(self):\n        self.assertEqual(answer(), 42)\n",
        )
        .unwrap();
        let result = verify_workspace(&Sandbox {
            root: dir.path().to_path_buf(),
        })
        .await
        .unwrap();
        assert!(result.passed, "{}", result.detail());
    }

    #[tokio::test]
    async fn rejects_invalid_javascript_syntax() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("broken.js"), "function {").unwrap();
        let result = verify_workspace(&Sandbox {
            root: dir.path().to_path_buf(),
        })
        .await
        .unwrap();
        assert!(!result.passed);
        assert!(result.detail().contains("javascript syntax"));
    }

    #[tokio::test]
    async fn verifies_rust_manifest_projects() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        .unwrap();
        let result = verify_workspace(&Sandbox {
            root: dir.path().to_path_buf(),
        })
        .await
        .unwrap();
        assert!(result.passed, "{}", result.detail());
    }
}
