use crate::sandbox::Sandbox;
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Tool {
    Shell,
    FileRead,
    FileWrite,
    WebGet,
    Script,
}
#[derive(Clone)]
pub struct ToolRegistry {
    client: Client,
    root: PathBuf,
}
impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
    fn safe_path(&self, p: &str) -> Result<PathBuf> {
        let path = Path::new(p);
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let canon = full.parent().unwrap_or(Path::new(".")).canonicalize()?;
        if !canon.starts_with(&self.root.canonicalize()?) {
            return Err(anyhow!("path escapes workspace"));
        }
        Ok(full)
    }
    pub async fn shell(&self, sandbox: &Sandbox, command: &str) -> Result<String> {
        let denied = [
            "`", "$(", " rm ", " rm\n", "sudo", "mkfs", "dd ", "chmod", "chown",
        ];
        let normalized = format!(" {command} ").to_ascii_lowercase();
        if command.trim().is_empty() {
            return Err(anyhow!("empty command"));
        }
        if command.len() > 512 || denied.iter().any(|token| normalized.contains(token)) {
            return Err(anyhow!("shell command rejected by safety policy"));
        }
        let out = sandbox.exec("sh", &["-lc", command]).await?;
        if out.status != 0 {
            return Err(anyhow!("command failed: {}", out.stderr));
        }
        if out.stderr.is_empty() {
            Ok(out.stdout)
        } else {
            Ok(format!("{}\nSTDERR:\n{}", out.stdout, out.stderr))
        }
    }
    pub async fn read_file(&self, path: &str) -> Result<String> {
        Ok(tokio::fs::read_to_string(self.safe_path(path)?).await?)
    }
    pub async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let p = self.safe_path(path)?;
        tokio::fs::write(p, content).await?;
        Ok(())
    }
    pub async fn web_get(&self, url: &str) -> Result<String> {
        let parsed = url::Url::parse(url)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(anyhow!("unsupported URL scheme"));
        }
        Ok(self
            .client
            .get(parsed)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    }
    pub async fn run_script(
        &self,
        sandbox: &Sandbox,
        language: &str,
        source: &str,
    ) -> Result<String> {
        let (prog, ext) = match language {
            "python" => ("python3", "py"),
            "node" => ("node", "js"),
            _ => return Err(anyhow!("unsupported language")),
        };
        let p = self.root.join(format!(".ozymandias-script.{ext}"));
        tokio::fs::write(&p, source).await?;
        let path = p.to_string_lossy().to_string();
        let out = sandbox.exec(prog, &[&path]).await?;
        let _ = tokio::fs::remove_file(&p).await;
        if out.status != 0 {
            return Err(anyhow!("script failed: {}", out.stderr));
        }
        Ok(out.stdout)
    }
}
