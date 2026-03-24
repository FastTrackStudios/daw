//! Method-ID-based handler composition for vox services.
//!
//! Routes incoming RPC calls to the correct service dispatcher by
//! matching `method_id` against each service's known methods.

use std::collections::HashMap;
use std::sync::Arc;
use vox::{
    Driver, DriverReplySink, Handler, MethodId, ReplySink, SchemaRecvTracker, SelfRef,
    ServiceDescriptor, VoxError,
};

/// A handler entry wrapping a concrete dispatcher behind a trait object.
trait DynHandler: Send + Sync + 'static {
    fn handle(
        &self,
        call: SelfRef<vox::RequestCall<'static>>,
        reply: DriverReplySink,
        schemas: Arc<SchemaRecvTracker>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>>;
}

impl<H: Handler<DriverReplySink>> DynHandler for H {
    fn handle(
        &self,
        call: SelfRef<vox::RequestCall<'static>>,
        reply: DriverReplySink,
        schemas: Arc<SchemaRecvTracker>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(Handler::handle(self, call, reply, schemas))
    }
}

/// Routes incoming calls to the correct service dispatcher by method_id.
#[derive(Clone)]
pub struct RoutedHandler {
    method_map: HashMap<MethodId, usize>,
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
    async fn handle(
        &self,
        call: SelfRef<vox::RequestCall<'static>>,
        reply: DriverReplySink,
        schemas: Arc<SchemaRecvTracker>,
    ) {
        let method_id = call.method_id;
        if let Some(&idx) = self.method_map.get(&method_id) {
            self.handlers[idx].handle(call, reply, schemas).await;
        } else {
            reply
                .send_error(VoxError::<core::convert::Infallible>::UnknownMethod)
                .await;
        }
    }
}
