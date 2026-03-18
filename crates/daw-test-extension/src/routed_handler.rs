//! Handler composition and connection acceptance for roam v7.
//!
//! - `RoutedHandler`: routes incoming calls to the correct service dispatcher
//!   by matching `method_id` against each service's known methods.
//!
//! - `DawConnectionAcceptor`: a `ConnectionAcceptor` that spawns a `Driver`
//!   with a `RoutedHandler` for each virtual connection. Guests open virtual
//!   connections with metadata to specify their role; the acceptor decides
//!   which services to expose.

use roam::{
    AcceptedConnection, ConnectionAcceptor, ConnectionHandle, ConnectionId, ConnectionSettings,
    Driver, DriverReplySink, Handler, MetadataEntry, MetadataValue, MethodId, ReplySink, RoamError,
    SelfRef, ServiceDescriptor,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

// ============================================================================
// RoutedHandler — method-ID-based dispatch
// ============================================================================

/// A handler entry wrapping a concrete dispatcher behind a trait object.
trait DynHandler: Send + Sync + 'static {
    fn handle(
        &self,
        call: SelfRef<roam::RequestCall<'static>>,
        reply: DriverReplySink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>>;
}

/// Blanket impl: any `Handler<DriverReplySink>` can be wrapped.
impl<H: Handler<DriverReplySink>> DynHandler for H {
    fn handle(
        &self,
        call: SelfRef<roam::RequestCall<'static>>,
        reply: DriverReplySink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(Handler::handle(self, call, reply))
    }
}

/// Routes incoming calls to the correct service dispatcher by method_id.
#[derive(Clone)]
pub struct RoutedHandler {
    /// method_id → index into `handlers`
    method_map: HashMap<MethodId, usize>,
    /// Concrete dispatchers, type-erased.
    handlers: Vec<Arc<dyn DynHandler>>,
}

impl RoutedHandler {
    pub fn new() -> Self {
        Self {
            method_map: HashMap::new(),
            handlers: Vec::new(),
        }
    }

    /// Register a service dispatcher with its known methods.
    pub fn with<H: Handler<DriverReplySink>>(
        mut self,
        descriptor: &ServiceDescriptor,
        handler: H,
    ) -> Self {
        let idx = self.handlers.len();
        self.handlers.push(Arc::new(handler));
        for method in descriptor.methods {
            self.method_map.insert(method.id, idx);
        }
        self
    }
}

impl Handler<DriverReplySink> for RoutedHandler {
    async fn handle(&self, call: SelfRef<roam::RequestCall<'static>>, reply: DriverReplySink) {
        let method_id = call.method_id;
        if let Some(&idx) = self.method_map.get(&method_id) {
            self.handlers[idx].handle(call, reply).await;
        } else {
            reply
                .send_error(RoamError::<core::convert::Infallible>::UnknownMethod)
                .await;
        }
    }
}

// ============================================================================
// DawConnectionAcceptor — virtual-connection-based service routing
// ============================================================================

/// Accepts inbound virtual connections and spawns a `Driver` with the
/// `RoutedHandler` for each one.
///
/// Guests open virtual connections with metadata to identify themselves.
/// Currently all connections get the full set of 16 DAW services.
/// Future: use metadata to restrict service sets per role.
#[derive(Clone)]
pub struct DawConnectionAcceptor {
    handler: Arc<RoutedHandler>,
}

impl DawConnectionAcceptor {
    pub fn new(handler: RoutedHandler) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }
}

impl ConnectionAcceptor for DawConnectionAcceptor {
    fn accept(
        &self,
        conn_id: ConnectionId,
        peer_settings: &ConnectionSettings,
        metadata: &[MetadataEntry],
    ) -> Result<AcceptedConnection, roam::Metadata<'static>> {
        let role = metadata
            .iter()
            .find(|e| e.key == "role")
            .and_then(|e| match e.value {
                MetadataValue::String(s) => Some(s),
                _ => None,
            })
            .unwrap_or("unknown");

        info!("Accepting virtual connection {}: role={}", conn_id, role);

        let handler = Arc::clone(&self.handler);
        let settings = ConnectionSettings {
            parity: peer_settings.parity.other(),
            max_concurrent_requests: 64,
        };

        Ok(AcceptedConnection {
            settings,
            metadata: vec![],
            setup: Box::new(move |handle: ConnectionHandle| {
                let mut driver = Driver::new(handle, handler.as_ref().clone());
                tokio::spawn(async move {
                    driver.run().await;
                });
            }),
        })
    }
}
