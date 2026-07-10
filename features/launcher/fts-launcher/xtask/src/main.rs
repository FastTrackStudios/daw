//! fts-launcher developer tasks — thin wrapper over the shared fts-repo battery.
use std::process::ExitCode;

fn main() -> ExitCode {
    let cfg = fts_repo::XtaskConfig {
        nextest_profile: "ci".into(),
        run_doctests: false,
        run_tracey: false,
        ..Default::default()
    };
    fts_repo::dispatch(&cfg, |_c, _r| None)
}
