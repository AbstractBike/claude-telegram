mod config;
mod matrix;
mod session;
mod sandbox;
mod agent;
mod secrets;
mod observability;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("claude-chat starting");
    Ok(())
}
