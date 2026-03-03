use tokio::process::Command;

pub struct BwrapBuilder {
    work_dir: String,
    store_dir: String,
}

impl BwrapBuilder {
    pub fn new(work_dir: impl Into<String>, store_dir: impl Into<String>) -> Self {
        Self {
            work_dir: work_dir.into(),
            store_dir: store_dir.into(),
        }
    }

    /// Returns the bwrap arguments (not including "bwrap" itself or the wrapped command)
    pub fn build_args(&self) -> Vec<String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let claude_config = format!("{home}/.claude");

        let mut args = vec![
            // Read-only system binds
            "--ro-bind".into(), "/nix".into(), "/nix".into(),
            "--ro-bind".into(), "/usr".into(), "/usr".into(),
            "--ro-bind".into(), "/etc/resolv.conf".into(), "/etc/resolv.conf".into(),
            // Minimal proc/dev
            "--proc".into(), "/proc".into(),
            "--dev".into(), "/dev".into(),
            // Ephemeral /tmp
            "--tmpfs".into(), "/tmp".into(),
            // Home directory (tmpfs base, then selective binds)
            "--tmpfs".into(), home.clone(),
            // Agent workdir (read-write)
            "--bind".into(), self.work_dir.clone(), self.work_dir.clone(),
            // Agent store (read-write, persistent state)
            "--bind".into(), self.store_dir.clone(), self.store_dir.clone(),
        ];

        // Bind .claude config read-only if it exists (needed for Claude CLI)
        if std::path::Path::new(&claude_config).exists() {
            args.extend_from_slice(&[
                "--ro-bind".into(), claude_config.clone(), claude_config,
            ]);
        }

        // Set HOME and working directory
        args.extend_from_slice(&[
            "--setenv".into(), "HOME".into(), home,
            "--chdir".into(), self.work_dir.clone(),
            // Isolation
            "--unshare-all".into(),
            "--share-net".into(),
            "--die-with-parent".into(),
        ]);

        args
    }

    /// Wrap a command with bwrap sandboxing
    pub fn wrap_command(&self, program: &str, program_args: &[&str]) -> Command {
        let mut cmd = Command::new("bwrap");
        cmd.args(self.build_args());
        cmd.arg("--");
        cmd.arg(program);
        cmd.args(program_args);
        cmd
    }
}
