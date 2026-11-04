use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Evaluation {
    pub passed: bool,
    pub detail: String,
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
