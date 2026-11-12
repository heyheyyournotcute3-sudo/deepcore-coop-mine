//! Lobby binary: the service lives in the `deepcore_lobby` library so tests can drive the
//! router directly.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    deepcore_lobby::run().await
}
