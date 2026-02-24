#[cfg(all(target_os = "macos", feature = "macos-endpoint-security"))]
mod daemon;

#[cfg(all(target_os = "macos", feature = "macos-endpoint-security"))]
pub use daemon::run_daemon;

#[cfg(not(all(target_os = "macos", feature = "macos-endpoint-security")))]
pub fn run_daemon() -> anyhow::Result<()> {
    anyhow::bail!("Endpoint Security daemon is only supported on macOS with 'macos-endpoint-security' feature enabled.");
}
