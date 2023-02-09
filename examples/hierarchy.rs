use std::time::Duration;

use futures::stream::{self, StreamExt};
use rand::thread_rng;
use rand::Rng;
use tracing::instrument;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[instrument]
async fn do_sub_work(val: u64) -> u64 {
    let sleep_time =
        thread_rng().gen_range(Duration::from_millis(1500)..Duration::from_millis(3000));
    tokio::time::sleep(sleep_time).await;

    val + 1
}

#[instrument]
async fn do_work(mut val: u64) -> u64 {
    let sleep_time = thread_rng().gen_range(Duration::from_millis(250)..Duration::from_millis(500));
    tokio::time::sleep(sleep_time).await;

    if thread_rng().gen_bool(0.2) {
        let (val1, val2) = tokio::join!(
            do_sub_work(val),
            do_sub_work(val),
        );

        val = val1 + val2;
    } else {
        val = do_sub_work(val).await;
    }

    let sleep_time =
        thread_rng().gen_range(Duration::from_millis(500)..Duration::from_millis(1000));
    tokio::time::sleep(sleep_time).await;

    val + 1
}

#[tokio::main]
async fn main() {
    let indicatif_layer = IndicatifLayer::new();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_writer()))
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
