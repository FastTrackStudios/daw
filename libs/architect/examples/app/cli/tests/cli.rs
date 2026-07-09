//! CLI integration tests.
//!
//! Spawns the real server (in-memory backend) in-process, then runs the
//! built `app` binary as a subprocess against it — exercising the actual
//! command-line surface end to end over vox, exit codes and all.

use app_server::vox_router;
use example::backend_memory::ExampleRepoMemory;
use tokio::process::Command;
use tokio::sync::oneshot;

async fn spawn() -> (String, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let app = vox_router(ExampleRepoMemory::new(), app_server::Collab::ephemeral());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (format!("ws://{addr}/vox"), tx)
}

fn app_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_app"))
}

/// Strip ANSI escape sequences so assertions match the plain text the
/// CLI colorizes for humans.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until the terminating 'm' of the CSI sequence.
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn uuid_in(s: &str) -> String {
    let plain = strip_ansi(s);
    plain
        .split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .find(|tok| tok.len() == 36 && tok.matches('-').count() == 4)
        .unwrap_or_else(|| panic!("no uuid in output: {plain:?}"))
        .to_string()
}

#[tokio::test]
async fn create_list_get_delete_flow() {
    let (url, shutdown) = spawn().await;

    // create --name --description
    let out = app_cmd()
        .args([
            "create",
            "--name",
            "cli-alpha",
            "--description",
            "via cli",
            "--url",
            &url,
        ])
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "create failed: {out:?}");
    let id = uuid_in(&String::from_utf8_lossy(&out.stdout));

    // list shows the row
    let out = app_cmd()
        .args(["list", "--url", &url])
        .output()
        .await
        .unwrap();
    let stdout = strip_ansi(&String::from_utf8_lossy(&out.stdout));
    assert!(out.status.success());
    assert!(stdout.contains("1 examples"), "list: {stdout:?}");
    assert!(stdout.contains("cli-alpha"), "list: {stdout:?}");

    // get <id> shows the fields
    let out = app_cmd()
        .args(["get", &id, "--url", &url])
        .output()
        .await
        .unwrap();
    let stdout = strip_ansi(&String::from_utf8_lossy(&out.stdout));
    assert!(out.status.success());
    assert!(stdout.contains("cli-alpha"), "get: {stdout:?}");
    assert!(stdout.contains("via cli"), "get: {stdout:?}");

    // delete <id>
    let out = app_cmd()
        .args(["delete", &id, "--url", &url])
        .output()
        .await
        .unwrap();
    assert!(out.status.success());

    // get after delete -> non-zero exit, NotFound on stderr
    let out = app_cmd()
        .args(["get", &id, "--url", &url])
        .output()
        .await
        .unwrap();
    assert!(!out.status.success(), "get-after-delete should fail");
    let stderr = strip_ansi(&String::from_utf8_lossy(&out.stderr));
    assert!(stderr.contains("NotFound"), "stderr: {stderr:?}");

    let _ = shutdown.send(());
}

#[tokio::test]
async fn create_without_description_defaults_empty() {
    let (url, shutdown) = spawn().await;
    let out = app_cmd()
        .args(["create", "--name", "only-name", "--url", &url])
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "create failed: {out:?}");
    let id = uuid_in(&String::from_utf8_lossy(&out.stdout));

    let out = app_cmd()
        .args(["get", &id, "--url", &url])
        .output()
        .await
        .unwrap();
    assert!(out.status.success());
    assert!(strip_ansi(&String::from_utf8_lossy(&out.stdout)).contains("only-name"));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn bad_uuid_is_a_clean_error() {
    let (url, shutdown) = spawn().await;
    let out = app_cmd()
        .args(["get", "not-a-uuid", "--url", &url])
        .output()
        .await
        .unwrap();
    assert!(!out.status.success(), "bad uuid should exit non-zero");
    let stderr = strip_ansi(&String::from_utf8_lossy(&out.stderr));
    assert!(stderr.to_lowercase().contains("uuid"), "stderr: {stderr:?}");
    let _ = shutdown.send(());
}
