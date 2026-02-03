//! Standalone DAW Cell Binary
//!
//! This is the entry point for running daw-standalone as a cell.
//! The actual implementations are in lib.rs for reuse in tests.
//!
//! ## Services Provided
//!
//! - **TransportService**: Play/pause/stop, position, tempo, looping
//! - **ProjectService**: Project and track management
//! - **MarkerService**: SONGSTART/SONGEND markers for song boundaries
//! - **RegionService**: Section regions (Intro, Verse, Chorus, etc.)
//! - **TempoMapService**: Tempo and time signature changes

use cell_runtime::run_cell;
use daw_proto::marker::MarkerServiceDispatcher;
use daw_proto::project::ProjectServiceDispatcher;
use daw_proto::region::RegionServiceDispatcher;
use daw_proto::tempo_map::TempoMapServiceDispatcher;
use daw_proto::transport::transport::TransportServiceDispatcher;
use daw_standalone::{
    StandaloneMarker, StandaloneProject, StandaloneRegion, StandaloneTempoMap, StandaloneTransport,
};
use roam_telemetry::{
    ExporterConfig, LoggingExporter, OtlpExporter, SpanExporter, TelemetryMiddleware,
};
use std::time::Duration;

/// Composite exporter that sends spans to both OTLP and logging
#[derive(Clone)]
struct CompositeExporter {
    otlp: OtlpExporter,
    logging: LoggingExporter,
}

impl SpanExporter for CompositeExporter {
    fn send(&self, span: roam_telemetry::Span) {
        self.logging.send(span.clone());
        self.otlp.send(span);
    }

    fn service_name(&self) -> &str {
        "daw-standalone"
    }
}

fn create_telemetry() -> TelemetryMiddleware<CompositeExporter> {
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4318/v1/traces".to_string());

    let otlp_exporter = OtlpExporter::with_config(ExporterConfig {
        endpoint: otlp_endpoint,
        service_name: "daw-standalone".to_string(),
        resource_attributes: vec![],
        max_batch_size: 10,
        max_batch_delay: Duration::from_secs(2),
        timeout: Duration::from_secs(10),
    });

    let logging_exporter = LoggingExporter::new("daw-standalone");

    let exporter = CompositeExporter {
        otlp: otlp_exporter,
        logging: logging_exporter,
    };

    TelemetryMiddleware::new(exporter)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_cell!("daw-standalone", |_handle| {
        let telemetry = create_telemetry();

        // Create service implementations
        // Project must be created first so we can share its state with transport
        let project = StandaloneProject::new();
        let transport = StandaloneTransport::new(project.shared_state());
        let marker = StandaloneMarker::new();
        let region = StandaloneRegion::new();
        let tempo_map = StandaloneTempoMap::new();

        // Create dispatchers with telemetry middleware
        let transport_dispatcher =
            TransportServiceDispatcher::new(transport).with_middleware(telemetry.clone());
        let project_dispatcher =
            ProjectServiceDispatcher::new(project).with_middleware(telemetry.clone());
        let marker_dispatcher =
            MarkerServiceDispatcher::new(marker).with_middleware(telemetry.clone());
        let region_dispatcher =
            RegionServiceDispatcher::new(region).with_middleware(telemetry.clone());
        let tempo_map_dispatcher =
            TempoMapServiceDispatcher::new(tempo_map).with_middleware(telemetry);

        // Compose all dispatchers together
        // The RoutedDispatcher chains dispatchers: first tries left, falls through to right
        let transport_project = RoutedDispatcher::new(project_dispatcher, transport_dispatcher);
        let with_marker = RoutedDispatcher::new(transport_project, marker_dispatcher);
        let with_region = RoutedDispatcher::new(with_marker, region_dispatcher);
        RoutedDispatcher::new(with_region, tempo_map_dispatcher)
    })
}
