use std::time::Duration;

use futures::stream::StreamExt;
use futures::stream::{self};
use rand::Rng;
use tracing::instrument;
use tracing_indicatif::IndicatifLayer;
use tracing_indicatif::filter::IndicatifFilter;
use tracing_indicatif::filter::hide_indicatif_span_fields;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[instrument(fields(indicatif.pb_show))]
async fn do_sub_work(val: u64) -> u64 {
    let sleep_time =
        rand::rng().random_range(Duration::from_millis(1500)..Duration::from_millis(3000));
    tokio::time::sleep(sleep_time).await;

    val + 1
}

#[instrument]
async fn do_work(mut val: u64) -> u64 {
    let sleep_time =
        rand::rng().random_range(Duration::from_millis(250)..Duration::from_millis(500));
    tokio::time::sleep(sleep_time).await;

    if rand::rng().random_bool(0.2) {
        let (val1, val2) = tokio::join!(do_sub_work(val), do_sub_work(val),);

        val = val1 + val2;
    } else {
        val = do_sub_work(val).await;
    }

    let sleep_time =
        rand::rng().random_range(Duration::from_millis(500)..Duration::from_millis(1000));
    tokio::time::sleep(sleep_time).await;

    val + 1
}

#[tokio::main]
async fn main() {
    let indicatif_layer = IndicatifLayer::new()
        .with_span_field_formatter(hide_indicatif_span_fields(DefaultFields::new()));

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer.with_filter(IndicatifFilter::new(false)))
        .init();

    let res: u64 = stream::iter((0..20).map(|val| do_work(val)))
        .buffer_unordered(5)
        .collect::<Vec<u64>>()
        .await
        .into_iter()
        .sum();

    println!("final result: {}", res);
}
