use std::time::Duration;

use futures::stream::StreamExt;
use futures::stream::{self};
use indicatif::ProgressStyle;
use rand::Rng;
use tracing::Span;
use tracing::info_span;
use tracing::instrument;
use tracing_indicatif::IndicatifLayer;
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[instrument]
async fn do_sub_work(val: u64) -> u64 {
    let sleep_time = rand::rng().random_range(Duration::from_secs(3)..Duration::from_secs(5));
    tokio::time::sleep(sleep_time).await;

    val + 1
}

#[instrument]
async fn do_work(mut val: u64) -> u64 {
    let sleep_time = rand::rng().random_range(Duration::from_secs(1)..Duration::from_secs(3));
    tokio::time::sleep(sleep_time).await;

    val = do_sub_work(val).await;

    val + 1
}

#[tokio::main]
async fn main() {
    let indicatif_layer = IndicatifLayer::new();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();

    let header_span = info_span!("header");
    header_span.pb_set_style(&ProgressStyle::with_template("{wide_bar} {pos}/{len} {msg}").unwrap());
    header_span.pb_set_length(20);
    header_span.pb_set_message("Processing items");
    header_span.pb_set_finish_message("All items processed");

    let header_span_enter = header_span.enter();

    let res: u64 = stream::iter((0..20).map(|val| async move {
        let res = do_work(val).await;
        Span::current().pb_inc(1);

        res
    }))
    .buffer_unordered(5)
    .collect::<Vec<u64>>()
    .await
    .into_iter()
    .sum();

    std::mem::drop(header_span_enter);
    std::mem::drop(header_span);

    println!("final result: {}", res);
}
