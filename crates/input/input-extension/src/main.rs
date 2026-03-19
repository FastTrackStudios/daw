//! Input extension — SHM guest for keyboard interception and keybinding dispatch.
//!
//! Connects to REAPER via daw-bridge SHM, subscribes to raw input events,
//! and processes them through the `input` crate's trie-based key dispatch.
//! Resolved actions are executed back on the host via `execute_action`.
//!
//! Placed in `UserPlugins/fts-extensions/` and hot-reloaded by daw-bridge.

mod vk_map;

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

    info!("[input:{pid}] Connected to REAPER via SHM");

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
    // The filter-upload pattern means TranslateAccel evaluates this locally
    // with no SHM round-trip per keypress.
    let input = daw.input();
    input
        .set_key_filter(daw::service::KeyFilter::EatAll)
        .await?;
    input.set_enabled(true).await?;
    info!("[input:{pid}] Input interception enabled (EatAll)");

    // Subscribe to input events from the host
    let mut rx = input.subscribe().await?;
    info!("[input:{pid}] Subscribed to input events");

    // Main event loop
    let mut processor = processor;
    let ctx = ActionContext::new();

    while let Ok(Some(event)) = rx.recv().await {
        match &*event {
            daw::service::InputEvent::Key(raw) => {
                // Only process KeyDown events (skip KeyUp, Char, etc.)
                if !matches!(
                    raw.msg_kind,
                    daw::service::KeyMsgKind::KeyDown | daw::service::KeyMsgKind::SysKeyDown
                ) {
                    continue;
                }

                let key_event = match vk_map::raw_to_key_event(raw) {
                    Some(ev) => ev,
                    None => {
                        debug!("[input:{pid}] Unmapped vk_code: {}", raw.vk_code);
                        continue;
                    }
                };

                let commands = processor.process(input::InputEvent::Key(key_event), &ctx);

                for cmd in commands {
                    match cmd {
                        InputCommand::Action(action_id) => {
                            info!("[input:{pid}] Execute: {action_id}");
                            input.execute_action(action_id.as_str()).await?;
                        }
                        InputCommand::ActionWithArgs { action, args } => {
                            info!("[input:{pid}] Execute: {action} (args: {args:?})");
                            input.execute_action(action.as_str()).await?;
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
                            // Pass unhandled keys back to REAPER by not doing anything.
                            // The key was already eaten by TranslateAccel — in the future
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

/// Load the input processor with keybinding configuration.
///
/// For now, creates an empty processor in Normal mode.
/// TODO: Load keybinding config from a `.styx` or `.json` file
/// in the REAPER resource path.
fn load_processor() -> InputProcessor {
    // Try loading from default config location
    // For now, return empty processor — keybindings will be loaded from config
    let config = KeymapConfig::default();
    match InputProcessor::from_config(config) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to load keymap config: {e}, using empty processor");
            InputProcessor::new()
        }
    }
}
