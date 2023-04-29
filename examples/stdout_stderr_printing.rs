use std::io::Write;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use rand::thread_rng;
use rand::Rng;
use tracing::instrument;
use tracing_indicatif::writer::get_indicatif_stderr_writer;
use tracing_indicatif::writer::get_indicatif_stdout_writer;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[instrument]
async fn do_work(val: u64) -> u64 {
    let sleep_time = thread_rng().gen_range(Duration::from_millis(250)..Duration::from_millis(500));
    tokio::time::sleep(sleep_time).await;

    writeln!(
        get_indicatif_stderr_writer().unwrap(),
        "writing val {} to stderr",
        val
    )
    .unwrap();

    writeln!(
        get_indicatif_stdout_writer().unwrap(),
        "writing val {} to stdout",
        val
    )
    .unwrap();

    let sleep_time =
        thread_rng().gen_range(Duration::from_millis(500)..Duration::from_millis(1000));
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
