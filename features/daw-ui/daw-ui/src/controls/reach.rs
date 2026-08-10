//! Getting hold of the track a control is about.
//!
//! Every write in this module family walks the same four steps — is there a
//! DAW, what is the current project, find the track by guid, act on it — and
//! each step has its own error. Written out per control it is five lines of
//! ceremony around one line of intent, repeated once per control, and each
//! copy is a chance to report a different thing when the connection is
//! missing.
//!
//! So it is written once. A control says what it wants done to its track and
//! nothing else:
//!
//! ```ignore
//! on_track(&guid, |t| async move { t.toggle_solo().await }).await
//! ```

use crate::prelude::*;

/// Run `f` against a track handle, or explain why there isn't one.
///
/// The errors are deliberately plain strings of context rather than a typed
/// error: nothing branches on *why* a write failed — there is nothing a
/// control could do differently — and every caller ends at the same
/// `tracing::warn!`.
pub async fn on_track<F, Fut>(guid: &str, f: F) -> eyre::Result<()>
where
    F: FnOnce(daw_control::TrackHandle) -> Fut,
    Fut: std::future::Future<Output = Result<(), daw_control::Error>>,
{
    let daw = daw_control::Daw::try_get().ok_or_else(|| eyre::eyre!("no DAW connected"))?;
    let project = daw.current_project().await?;
    let track = project
        .tracks()
        .by_guid(guid)
        .await?
        .ok_or_else(|| eyre::eyre!("no track {guid}"))?;
    f(track).await?;
    Ok(())
}

/// Fire `f` at the DAW and log whatever it says.
///
/// The shape every click handler wants: a control has already updated what
/// it draws (or is waiting on the event that confirms the write), so there
/// is nothing to await and nothing to do with a failure but say so.
pub fn write_track<F, Fut>(guid: String, what: &'static str, f: F)
where
    F: FnOnce(daw_control::TrackHandle) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<(), daw_control::Error>> + 'static,
{
    spawn(async move {
        if let Err(e) = on_track(&guid, f).await {
            tracing::warn!("{what} {guid}: {e}");
        }
    });
}
