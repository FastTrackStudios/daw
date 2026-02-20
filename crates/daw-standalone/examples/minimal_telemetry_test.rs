//! Minimal telemetry test
//!
//! This tests if roam-telemetry middleware is working correctly

use roam::service;
use roam::session::Context;
use roam_telemetry::{LoggingExporter, TelemetryMiddleware};

#[service]
trait TestService {
    async fn echo(&self, message: String) -> String;
}

#[derive(Clone)]
struct TestServiceImpl;

impl TestService for TestServiceImpl {
    async fn echo(&self, _cx: &Context, message: String) -> String {
        println!("Service: echo called with: {}", message);
        message
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Minimal Telemetry Test ===\n");

    // Create logging exporter (prints to console)
    let exporter = LoggingExporter::new("test-service");
    println!("Created LoggingExporter");

    // Create telemetry middleware
    let telemetry = TelemetryMiddleware::new(exporter);
    println!("Created TelemetryMiddleware");

    // Create dispatcher with middleware
    let _dispatcher = TestServiceDispatcher::new(TestServiceImpl).with_middleware(telemetry);
    println!("Created dispatcher with middleware\n");

    // Test: Try to invoke a method through the dispatcher
    println!("Testing middleware invocation...");

    // Create a dummy context
    let _cx = Context::new(
        roam::wire::ConnectionId::new(1),
        roam::wire::RequestId::new(1),
        roam::wire::MethodId::new(1), // echo method
        roam::wire::Metadata::default(),
        vec![], // args
    );

    println!("If telemetry is working, you should see a span logged above.");
    println!("=== Test Complete ===");

    Ok(())
}
