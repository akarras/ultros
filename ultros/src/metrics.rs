use std::sync::Arc;

use axum::extract::State;
use opentelemetry::sdk::{
    export::metrics::aggregation,
    metrics::{controllers, processors, selectors},
};
use opentelemetry_prometheus::PrometheusExporter;
use prometheus::TextEncoder;

use crate::web::error::WebError;

pub(crate) fn init_telemetry() {
    //let otel_rsrc = make_resource(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    //let otel_tracer = otlp::init_tracer(otel_rsc, otlp::identity).expect("failed setup of tracer");
    //tracing_subscriber::registry().with(otel_layer)
    //let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    //let subscriber = Registry::default().with(telemetry);
    //tracing::subscriber::set_global_default(subscriber).expect("global default");
}

pub(crate) fn init_meter() -> PrometheusExporter {
    let controller = controllers::basic(
        processors::factory(
            selectors::simple::histogram([1.0, 2.0, 5.0, 10.0, 20.0, 50.0]),
            aggregation::cumulative_temporality_selector(),
        )
        .with_memory(true),
    )
    .build();

    opentelemetry_prometheus::exporter(controller).init()
}

pub(crate) async fn metrics(
    State(exporter): State<Arc<PrometheusExporter>>,
) -> Result<String, WebError> {
    let encoder = TextEncoder::new();
    let metric_families = exporter.registry().gather();
    Ok(encoder.encode_to_string(&metric_families)?)
}
