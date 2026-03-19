//! Input extension — SHM guest for keyboard interception and keybinding dispatch.
//!
//! Connects to the DAW host via daw-bridge SHM, subscribes to input events,
//! and processes them through the `input` crate's trie-based key dispatch.
//! Resolved actions are executed back on the host via `execute_action`.
//!
//! This extension is DAW-agnostic — all platform-specific key conversion
//! happens in the host (daw-bridge). The wire format uses `KeyCode`.
//!
//! Placed in `UserPlugins/fts-extensions/` and hot-reloaded by daw-bridge.

use daw_extension_runtime::GuestOptions;
use eyre::Result;
use input::{ActionContext, InputCommand, InputProcessor, KeymapConfig};
use tracing::{debug, info, warn};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(run())
}

async fn run() -> Result<()> {
    let pid = std::process::id();
    info!("[input:{pid}] Input extension starting");

    let daw = daw_extension_runtime::connect(GuestOptions {
        role: "input",
        ..Default::default()
    })
    .await?;

    info!("[input:{pid}] Connected to DAW host via SHM");

    // Health beacon — tests and the host can verify the extension is alive
    daw.ext_state()
        .set("FTS_INPUT_EXT", "status", "ready", false)
        .await?;
    daw.ext_state()
        .set("FTS_INPUT_EXT", "pid", &pid.to_string(), false)
        .await?;
    info!("[input:{pid}] Health beacon written");

    // Load keybinding config
    let processor = load_processor();
    info!(
        "[input:{pid}] Input processor loaded (mode: {:?})",
        processor.current_mode()
    );

    // Enable input interception and set filter to EatAll.
    // The filter-upload pattern means the host's keyboard hook evaluates
    // this locally with no SHM round-trip per keypress.
    let input_handle = daw.input();
    input_handle
        .set_key_filter(daw::service::KeyFilter::EatAll)
        .await?;
    input_handle.set_enabled(true).await?;
    info!("[input:{pid}] Input interception enabled (EatAll)");

    // Subscribe to input events from the host
    let mut rx = input_handle.subscribe().await?;
    info!("[input:{pid}] Subscribed to input events");

    // Main event loop
    let mut processor = processor;
    let ctx = ActionContext::new();

    while let Ok(Some(event)) = rx.recv().await {
        match &*event {
            daw::service::InputEvent::Key(key_event) => {
                // Only process KeyDown events (skip KeyUp, Char, etc.)
                if !matches!(
                    key_event.msg_kind,
                    daw::service::KeyMsgKind::KeyDown | daw::service::KeyMsgKind::SysKeyDown
                ) {
                    continue;
                }

                // Convert daw_proto::KeyEvent → input::KeyEvent
                // Both use the same agnostic key representation, so this is trivial.
                let input_key_event = to_input_key_event(key_event);

                let commands = processor.process(input::InputEvent::Key(input_key_event), &ctx);

                for cmd in commands {
                    match cmd {
                        InputCommand::Action(action_id) => {
                            info!("[input:{pid}] Execute: {action_id}");
                            input_handle.execute_action(action_id.as_str()).await?;
                        }
                        InputCommand::ActionWithArgs { action, args } => {
                            info!("[input:{pid}] Execute: {action} (args: {args:?})");
                            input_handle.execute_action(action.as_str()).await?;
                        }
                        InputCommand::SwitchMode(mode) => {
                            info!("[input:{pid}] Mode → {mode:?}");
                        }
                        InputCommand::PushMode(mode) => {
                            info!("[input:{pid}] Push mode: {mode:?}");
                        }
                        InputCommand::PopMode => {
                            info!("[input:{pid}] Pop mode");
                        }
                        InputCommand::Pending { display: pending } => {
                            debug!("[input:{pid}] Pending: {pending}");
                        }
                        InputCommand::Unhandled(_) => {
                            // The key was already eaten by the host — in the future
                            // we could implement a "pass-back" mechanism.
                        }
                        InputCommand::InsertText(text) => {
                            debug!("[input:{pid}] Insert text: {text}");
                        }
                    }
                }
            }
            daw::service::InputEvent::MouseWheel {
                delta,
                horizontal,
                context,
            } => {
                debug!(
                    "[input:{pid}] Mouse wheel: delta={delta} horizontal={horizontal} context={context:?}"
                );
            }
        }
    }

    info!("[input:{pid}] Input event stream ended");
    Ok(())
}

/// Convert a `daw_proto::KeyEvent` to an `input::KeyEvent`.
///
/// Both use platform-agnostic key representations with matching variants,
/// so this is a straightforward mapping.
fn to_input_key_event(ev: &daw::service::KeyEvent) -> input::KeyEvent {
    input::KeyEvent {
        key: to_input_keycode(&ev.key),
        modifiers: input::Modifiers {
            ctrl: ev.modifiers.ctrl,
            alt: ev.modifiers.alt,
            shift: ev.modifiers.shift,
            meta: false,
        },
    }
}

/// Convert `daw_proto::KeyCode` → `input::KeyCode`.
fn to_input_keycode(k: &daw::service::KeyCode) -> input::KeyCode {
    match k {
        daw::service::KeyCode::Character(c) => input::KeyCode::Character(c.clone()),
        daw::service::KeyCode::ArrowUp => input::KeyCode::ArrowUp,
        daw::service::KeyCode::ArrowDown => input::KeyCode::ArrowDown,
        daw::service::KeyCode::ArrowLeft => input::KeyCode::ArrowLeft,
        daw::service::KeyCode::ArrowRight => input::KeyCode::ArrowRight,
        daw::service::KeyCode::Enter => input::KeyCode::Enter,
        daw::service::KeyCode::Escape => input::KeyCode::Escape,
        daw::service::KeyCode::Tab => input::KeyCode::Tab,
        daw::service::KeyCode::Backspace => input::KeyCode::Backspace,
        daw::service::KeyCode::Delete => input::KeyCode::Delete,
        daw::service::KeyCode::Home => input::KeyCode::Enter, // input crate lacks Home
        daw::service::KeyCode::End => input::KeyCode::Enter,  // input crate lacks End
        daw::service::KeyCode::PageUp => input::KeyCode::Enter, // input crate lacks PageUp
        daw::service::KeyCode::PageDown => input::KeyCode::Enter, // input crate lacks PageDown
        daw::service::KeyCode::Insert => input::KeyCode::Enter, // input crate lacks Insert
        daw::service::KeyCode::F(n) => input::KeyCode::F(*n),
    }
}

/// Load the input processor with keybinding configuration.
///
/// For now, creates an empty processor in Normal mode.
/// TODO: Load keybinding config from a config file.
fn load_processor() -> InputProcessor {
    let config = KeymapConfig::default();
    match InputProcessor::from_config(config) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to load keymap config: {e}, using empty processor");
            InputProcessor::new()
        }
    }
}
