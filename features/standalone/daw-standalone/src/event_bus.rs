//! `impl EventBus for Standalone`.

use crate::Standalone;
use daw_proto::event_bus::{BusFilter, DawEvent, EventBus};
use daw_proto::fx::FxStreamEvent;
use daw_proto::marker::MarkerStreamEvent;
use daw_proto::region::RegionStreamEvent;
use daw_proto::tempo_map::TempoMapStreamEvent;
use daw_proto::track::TrackStreamEvent;
use tokio::sync::broadcast::error::RecvError;
use vox::Tx;

impl EventBus for Standalone {
    async fn subscribe(&self, filter: BusFilter, tx: Tx<DawEvent>) {
        if !filter.any() {
            return;
        }

        let mut track_rx = filter.tracks.then(|| self.track_events.subscribe());
        let mut fx_rx = filter.fx.then(|| self.fx_events.subscribe());
        let mut marker_rx = filter.markers.then(|| self.marker_events.subscribe());
        let mut region_rx = filter.regions.then(|| self.region_events.subscribe());
        let mut tempo_rx = filter.tempo_map.then(|| self.tempo_map_events.subscribe());

        // Transport state + position aren't broadcast — they live in
        // per-subscriber pumps on the transport engine. Bridge one
        // internal subscription onto the bus.
        let mut transport_rx = if filter.transport_state || filter.transport_position {
            use daw_proto::transport::service::Transport as TransportService;
            let (ttx, trx) = vox::channel();
            TransportService::subscribe(
                self,
                daw_proto::ProjectContext::Current,
                daw_proto::transport::TransportSubscription {
                    state: filter.transport_state,
                    position: filter.transport_position,
                },
                ttx,
            )
            .await;
            Some(trx)
        } else {
            None
        };

        moire::task::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    res = async { transport_rx.as_mut().unwrap().recv().await }, if transport_rx.is_some() => {
                        match res {
                            Ok(Some(ev)) => {
                                let mut owned = None;
                                let _ = ev.map(|e| owned = Some(e));
                                let wrapped = match owned.expect("SelfRef::map runs once") {
                                    daw_proto::transport::TransportStreamEvent::State(s) =>
                                        DawEvent::TransportState(s),
                                    daw_proto::transport::TransportStreamEvent::Position(p) =>
                                        DawEvent::TransportPosition(p),
                                };
                                if tx.send(wrapped).await.is_err() {
                                    return;
                                }
                            }
                            Ok(None) | Err(_) => transport_rx = None,
                        }
                    }
                    res = async { track_rx.as_mut().unwrap().recv().await }, if track_rx.is_some() => {
                        if !forward(&tx, res, &filter, track_guid, DawEvent::Track, &mut track_rx).await {
                            return;
                        }
                    }
                    res = async { fx_rx.as_mut().unwrap().recv().await }, if fx_rx.is_some() => {
                        if !forward(&tx, res, &filter, fx_guid, DawEvent::Fx, &mut fx_rx).await {
                            return;
                        }
                    }
                    res = async { marker_rx.as_mut().unwrap().recv().await }, if marker_rx.is_some() => {
                        if !forward(&tx, res, &filter, marker_guid, DawEvent::Marker, &mut marker_rx).await {
                            return;
                        }
                    }
                    res = async { region_rx.as_mut().unwrap().recv().await }, if region_rx.is_some() => {
                        if !forward(&tx, res, &filter, region_guid, DawEvent::Region, &mut region_rx).await {
                            return;
                        }
                    }
                    res = async { tempo_rx.as_mut().unwrap().recv().await }, if tempo_rx.is_some() => {
                        if !forward(&tx, res, &filter, tempo_guid, DawEvent::TempoMap, &mut tempo_rx).await {
                            return;
                        }
                    }
                }

                if track_rx.is_none()
                    && fx_rx.is_none()
                    && marker_rx.is_none()
                    && region_rx.is_none()
                    && tempo_rx.is_none()
                    && transport_rx.is_none()
                {
                    return;
                }
            }
        });
    }
}

fn track_guid(e: &TrackStreamEvent) -> &str {
    &e.project_guid
}

fn fx_guid(e: &FxStreamEvent) -> &str {
    &e.project_guid
}

fn marker_guid(e: &MarkerStreamEvent) -> &str {
    &e.project_guid
}

fn region_guid(e: &RegionStreamEvent) -> &str {
    &e.project_guid
}

fn tempo_guid(e: &TempoMapStreamEvent) -> &str {
    &e.project_guid
}

async fn forward<T, R>(
    tx: &Tx<DawEvent>,
    res: Result<T, RecvError>,
    filter: &BusFilter,
    extract_guid: fn(&T) -> &str,
    wrap: impl FnOnce(T) -> DawEvent,
    slot: &mut Option<R>,
) -> bool {
    match res {
        Ok(event) => {
            if filter.project_rejects(extract_guid(&event)) {
                return true;
            }
            tx.send(wrap(event)).await.is_ok()
        }
        Err(RecvError::Closed) => {
            *slot = None;
            true
        }
        Err(RecvError::Lagged(_)) => true,
    }
}
