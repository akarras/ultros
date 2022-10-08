use std::sync::Arc;

use axum::extract::State;
use opentelemetry::sdk::{
    export::{metrics::aggregation, trace::stdout},
    metrics::{controllers, processors, selectors},
};
use opentelemetry_prometheus::PrometheusExporter;
use prometheus::{Encoder, TextEncoder};
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, Registry};

use crate::web::error::WebError;

pub(crate) fn init_telemetry() {
    let tracer = stdout::new_pipeline().install_simple();

    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(telemetry);
    tracing::subscriber::set_global_default(subscriber).expect("global default");
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
