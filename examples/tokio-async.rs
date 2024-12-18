#![feature(mpmc_channel)]

use std::{error::Error, time::Duration};

use indicatif::ProgressStyle;
use rand::random;
use tracing::{info, instrument, Level, Span};
use tracing_core::LevelFilter;
use tracing_indicatif::{span_ext::IndicatifSpanExt, IndicatifLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

type Result<T> = std::result::Result<T, Box<dyn Error + Sync + Send>>;

const WORKER_NUM: Option<&str> = option_env!("WORKER_NUM");

#[tokio::main]
async fn main() -> Result<()> {
    let indicatif_layer = IndicatifLayer::new()
        .with_progress_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} {msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
                )?
                .progress_chars("#>-"),
        )
        .with_max_progress_bars(u64::MAX, None);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .with_filter(LevelFilter::from_level(Level::INFO)),
        )
        .with(indicatif_layer)
        .init();

    let (tx, rx) = std::sync::mpmc::channel();

    let mut task_set = tokio::task::JoinSet::new();
    for _ in 0..WORKER_NUM.unwrap_or("4").parse()? {
        let rx = rx.clone();
        task_set.spawn(async move {
            while let Ok(_) = rx.recv() {
                payload().await
            }
        });
    }
    drop(rx);

    for _ in 0..10 {
        let _ = tx.send(());
    }
    drop(tx);

    task_set.join_all().await;

    Ok(())
}

#[instrument]
async fn payload() {
    let target: u32 = random::<u16>() as u32 * 1000;

    Span::current().pb_set_length(target as _);

    let mut cur: u32 = 0;
    let speed = 1024 * 1024;
    info!("Hello there");
    loop {
        cur += speed;

        Span::current().pb_set_position(cur.min(target) as _);
        if cur >= target {
            break;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
