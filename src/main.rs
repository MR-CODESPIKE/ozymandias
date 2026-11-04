use anyhow::Result;
use ozymandias::agent::AutonomousAgent;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let objective = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let objective = if objective.is_empty() {
        "Inspect the local runtime and summarize available capabilities.".to_string()
    } else {
        objective
    };
    let result = AutonomousAgent::new(16).solve(&objective).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
