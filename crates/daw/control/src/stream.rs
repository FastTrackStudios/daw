//! Client-side plumbing for architect `#[subscribe]` streams.
//!
//! A `#[subscribe]` stream is argless on the wire — the server sends
//! every event and the client filters. [`EventStream`] wraps the raw
//! `vox::Rx` from the stream call with an admit predicate (project
//! guid, `BusFilter`, …) and owns the in-flight subscribe call: the
//! call future never completes while the subscription lives (vox
//! scopes channels to their request), so it's parked on a spawned
//! task that is released when the `EventStream` is dropped —
//! cancelling the call is the unsubscribe.

use core::future::Future;

/// A live, client-filtered event subscription. Yields events exactly
/// like `vox::Rx::recv` (`Ok(Some(SelfRef<T>))` per event, `Ok(None)`
/// / `Err` when the stream ends); events rejected by the filter are
/// skipped transparently. Dropping the stream unsubscribes.
pub struct EventStream<T: 'static> {
    rx: vox::Rx<T>,
    admit: Box<dyn Fn(&T) -> bool + Send + Sync>,
    /// Dropping this releases the spawned task holding the subscribe
    /// call in flight, cancelling the server-side subscription.
    _stop: futures::channel::oneshot::Sender<()>,
}

impl<T> EventStream<T>
where
    T: facet::Facet<'static> + for<'a> vox_types::Reborrow<Ref<'a> = T> + Send + 'static,
{
    /// Park `call` (the in-flight `events(tx)` future) on a task and
    /// wrap `rx` with `admit`.
    pub(crate) fn spawn(
        call: impl Future<Output = ()> + Send + 'static,
        rx: vox::Rx<T>,
        admit: Box<dyn Fn(&T) -> bool + Send + Sync>,
    ) -> Self {
        let (stop_tx, stop_rx) = futures::channel::oneshot::channel::<()>();
        moire::task::spawn(async move {
            futures::pin_mut!(call);
            // Ends when the call ends (connection gone) or the
            // EventStream is dropped (stop_rx resolves) — either way
            // the call future is dropped, which is the unsubscribe.
            let _ = futures::future::select(call, stop_rx).await;
        });
        Self {
            rx,
            admit,
            _stop: stop_tx,
        }
    }

    /// Next admitted event. Same contract as `vox::Rx::recv`.
    pub async fn recv(&mut self) -> Result<Option<vox::SelfRef<T>>, vox::RxError> {
        loop {
            match self.rx.recv().await? {
                None => return Ok(None),
                Some(ev) => {
                    if (self.admit)(ev.get()) {
                        return Ok(Some(ev));
                    }
                }
            }
        }
    }
}

impl<T> std::fmt::Debug for EventStream<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}
