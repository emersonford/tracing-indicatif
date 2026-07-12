use futures::stream::StreamExt;
use futures::stream::{self};
use indicatif::MultiProgress;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rand::Rng;
use std::time::Duration;
use tracing::info;
use tracing::instrument;
use tracing_indicatif::IndicatifLayer;
use tracing_indicatif::TickSettings;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[instrument]
async fn do_work(val: u64) -> u64 {
    let sleep_time =
        rand::rng().random_range(Duration::from_millis(250)..Duration::from_millis(500));
    tokio::time::sleep(sleep_time).await;

    info!("doing work for val: {val}");

    let sleep_time =
        rand::rng().random_range(Duration::from_millis(500)..Duration::from_millis(1000));
    tokio::time::sleep(sleep_time).await;

    val + 1
}

#[tokio::main]
async fn main() {
    let mp = MultiProgress::new();

    // Add an external progress bar that's not managed by tracing
    let external_pb = mp
        .add(ProgressBar::new(100))
        .with_message("Task Running...");

    external_pb.set_style(
        ProgressStyle::with_template("[{bar:40}] {pos}/{len} {msg}")
            .expect("valid template")
            .progress_chars("=>-"),
    );

    let x = tokio::spawn({
        let mp = mp.clone();
        let pb = external_pb.clone();

        async move {
            for i in 0..=100 {
                pb.set_position(i);

                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            pb.finish_with_message("Task completed");

            mp.remove(&pb);
        }
    });

    // Create the indicatif layer using the builder with custom settings
    let indicatif_layer = IndicatifLayer::builder()
        .with_multi_progress(mp.clone())
        .with_max_progress_bars(5)
        .with_footer_style(Some(
            ProgressStyle::with_template("⏳ + {pending_progress_bars} more...")
                .expect("valid template"),
        ))
        .with_progress_style(
            ProgressStyle::with_template(
                "{span_child_prefix}{spinner:.cyan} {span_name}{{{span_fields}}}",
            )
            .unwrap(),
        )
        .with_tick_settings(TickSettings {
            term_draw_hz: 30,
            default_tick_interval: Some(Duration::from_millis(50)),
            ..Default::default()
        })
        .build();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();

    let res: u64 = stream::iter((0..20).map(|val| do_work(val)))
        .buffer_unordered(10)
        .collect::<Vec<u64>>()
        .await
        .into_iter()
        .sum();

    mp.println(format!("tracing result: {res}")).unwrap();

    x.await.unwrap();
}
