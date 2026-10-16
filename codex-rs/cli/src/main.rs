#[cfg(all(
    target_os = "linux",
    target_env = "musl",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() -> anyhow::Result<()> {
    #[cfg(unix)]
    if let Some(token) = std::env::var_os("OSC7577_TOKEN") {
        // SAFETY: This runs at process entry, before starting any other threads.
        unsafe { std::env::remove_var("OSC7577_TOKEN") };
        if let Ok(token) = token.into_string() {
            codex_tui::set_program_resume_token(token);
        }
    }
    codex_build_info::initialize!();
    codex_cli::run()
}
