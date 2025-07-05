use std::time::Duration;

use futures::stream::StreamExt;
use futures::stream::{self};
use rand::Rng;
use tracing::info;
use tracing::instrument;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[instrument]
async fn do_work(val: u64) -> u64 {
    let sleep_time =
        rand::rng().random_range(Duration::from_millis(250)..Duration::from_millis(500));
    tokio::time::sleep(sleep_time).await;

    info!("doing work for val: {}", val);

    let sleep_time =
        rand::rng().random_range(Duration::from_millis(500)..Duration::from_millis(1000));
    tokio::time::sleep(sleep_time).await;

    val + 1
}

#[tokio::main]
async fn main() {
    let indicatif_layer = IndicatifLayer::new();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();

    let res: u64 = stream::iter((0..20).map(|val| do_work(val)))
        .buffer_unordered(5)
        .collect::<Vec<u64>>()
        .await
        .into_iter()
        .sum();

    println!("final result: {}", res);
}
