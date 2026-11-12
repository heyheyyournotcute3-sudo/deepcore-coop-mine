//! Binary entry point: the server lives in the `deepcore_server` library so integration tests
//! can drive its systems directly.

fn main() -> anyhow::Result<()> {
    deepcore_server::run()
}
