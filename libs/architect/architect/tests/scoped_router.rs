//! Instance-scoped service mounting, end-to-end over an in-process
//! `LocalServer`: the same `#[architect::rpc]` trait mounted twice on one
//! router under two scopes; each scoped client reaches its own instance, and
//! an unscoped mount keeps serving unscoped calls.
#![cfg(all(feature = "local", not(target_arch = "wasm32")))]

use architect::local::LocalServer;
use architect::resource::Scope;
use architect::{LayerRouter, Services, layers};

mod echo_proto {
    #[architect::rpc]
    pub trait Echo {
        /// Which instance answered.
        fn whoami(&self) -> String;
    }
}

use echo_proto::prelude::EchoService;
use echo_proto::EchoClient;

#[derive(Clone, architect::HasDispatcher)]
#[dispatch(architect::dispatch::CurrentThreadDispatcher)]
struct EchoBackend {
    name: &'static str,
}

impl echo_proto::Echo for EchoBackend {
    fn whoami(&self) -> String {
        self.name.to_string()
    }
}

impl Services for EchoBackend {
    fn layers() -> impl architect::Layer<Self> {
        layers![EchoService]
    }
}

fn router_for(name: &'static str) -> LayerRouter {
    use architect::Services as _;
    EchoBackend { name }.into_router()
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_mounts_dispatch_by_scope() {
    // One merged router: an unscoped default plus two scoped instances of
    // the SAME service trait (which would otherwise collide, last-merge
    // wins).
    let router = router_for("default")
        .merge_router_scoped("guitar", router_for("guitar"))
        .merge_router_scoped("keys", router_for("keys"));
    let server = LocalServer::serve(router, Scope::new());

    // Unscoped client → the flat mount.
    let plain: EchoClient = server.establish().await.expect("establish");
    assert_eq!(plain.whoami().await.expect("call"), "default");

    // Scoped clients → their instances.
    let guitar: EchoClient = server.establish().await.expect("establish");
    let guitar = architect::scope_client!(guitar, "guitar");
    assert_eq!(guitar.whoami().await.expect("call"), "guitar");

    let keys: EchoClient = server.establish().await.expect("establish");
    let keys = architect::scope_client!(keys, "keys");
    assert_eq!(keys.whoami().await.expect("call"), "keys");

    // An unknown scope falls back to the flat mount (graceful, not an
    // UnknownMethod error).
    let stray: EchoClient = server.establish().await.expect("establish");
    let stray = architect::scope_client!(stray, "bass");
    assert_eq!(stray.whoami().await.expect("call"), "default");
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_only_router_serves_scoped_calls() {
    // No unscoped mount at all — shape queries must still resolve through
    // the scoped entries (same trait ⇒ same shapes).
    let router = LayerRouter::new().merge_router_scoped("keys", router_for("keys"));
    let server = LocalServer::serve(router, Scope::new());

    let keys: EchoClient = server.establish().await.expect("establish");
    let keys = architect::scope_client!(keys, "keys");
    assert_eq!(keys.whoami().await.expect("call"), "keys");
}

/// The generated **direct view**: inherent methods over a local backend —
/// no trait import at the call site, no UFCS even when method names collide
/// across services.
#[test]
fn direct_view_calls_without_ufcs() {
    use echo_proto::EchoDirectExt as _;
    let backend = EchoBackend { name: "direct" };
    // `echo_direct()` comes from the generated blanket ext trait; `whoami`
    // is an inherent method on the view, so no `Echo` import is needed.
    assert_eq!(backend.echo_direct().whoami(), "direct");
    // The view is Copy — cheap to pass around.
    let view = backend.echo_direct();
    let (a, b) = (view, view);
    assert_eq!(a.whoami(), b.whoami());
}
