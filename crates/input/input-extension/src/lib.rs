//! Input integrated REAPER extension.
//!
//! Loaded directly by REAPER from `UserPlugins/`. Subscribes to input events
//! and processes them through the `input` crate's trie-based key dispatch.
//! Resolved actions are executed back on the host via `execute_action`.

use std::cell::OnceCell;
use std::error::Error;

use daw::Daw;
use daw_extension_runtime::ExtensionRuntime;
use eyre::Result;
use fragile::Fragile;
use input::{ActionContext, InputCommand, InputProcessor, KeymapConfig};
use reaper_low::PluginContext;
use reaper_macros::reaper_extension_plugin;
use tracing::{debug, info, warn};

thread_local! {
    static APP: OnceCell<Fragile<InputExtension>> = const { OnceCell::new() };
}

struct InputExtension {
    runtime: ExtensionRuntime,
}

impl InputExtension {
    fn new(context: PluginContext) -> Result<Self> {
        let runtime = ExtensionRuntime::new(context)?;
        let daw = runtime.build_daw()?;

        runtime.spawn(async move {
            if let Err(e) = run(daw).await {
                warn!("[input] event loop ended: {e}");
            }
        });

        Ok(Self { runtime })
    }

    fn timer(&self) {
        self.runtime.process_tasks();
    }
}

extern "C" fn timer_callback() {
    APP.with(|cell| {
        if let Some(app) = cell.get() {
            app.get().timer();
        }
    });
}

#[reaper_extension_plugin]
fn plugin_main(context: PluginContext) -> std::result::Result<(), Box<dyn Error>> {
    init_tracing();
    info!("input-extension starting");

    let app = InputExtension::new(context).map_err(|e| -> Box<dyn Error> { e.into() })?;
    app.runtime.add_timer(timer_callback).map_err(|e| -> Box<dyn Error> { e.into() })?;

    let stored = APP.with(|cell| cell.set(Fragile::new(app)).is_ok());
    if !stored {
        return Err("input-extension already initialized".into());
    }

    info!("input-extension loaded");
    Ok(())
}

fn init_tracing() {
    let Ok(log_file) = std::fs::File::create("/tmp/input-extension.log") else {
        return;
    };
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

async fn run(daw: Daw) -> Result<()> {
    let pid = std::process::id();
    info!("[input:{pid}] runtime started");

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
        match event.get() {
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
                let input_key_event = to_input_key_event(&key_event);

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
