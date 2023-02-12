use std::time::Duration;

use futures::stream::{self, StreamExt};
use indicatif::ProgressState;
use indicatif::ProgressStyle;
use rand::thread_rng;
use rand::Rng;
use tracing::info;
use tracing::instrument;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

// TODO(emersonford): can we show a "header" to the progress bars like superconsole does?
// https://github.com/facebookincubator/superconsole

#[instrument]
async fn build_sub_unit(sub_unit: u64) {
    let sleep_time =
        thread_rng().gen_range(Duration::from_millis(5000)..Duration::from_millis(10000));
    tokio::time::sleep(sleep_time).await;

    if thread_rng().gen_bool(0.2) {
        info!("sub_unit did something!");
    }
}

#[instrument]
async fn build(unit: u64) {
    let sleep_time =
        thread_rng().gen_range(Duration::from_millis(2500)..Duration::from_millis(5000));
    tokio::time::sleep(sleep_time).await;

    let rand_num: f64 = thread_rng().gen();

    if rand_num < 0.1 {
        tokio::join!(build_sub_unit(0), build_sub_unit(1), build_sub_unit(2));
    } else if rand_num < 0.3 {
        tokio::join!(build_sub_unit(0), build_sub_unit(1));
    } else {
        build_sub_unit(0).await;
    }
}

#[tokio::main]
async fn main() {
    let indicatif_layer = IndicatifLayer::new().with_progress_style(
        ProgressStyle::with_template(
            "{color_start}{span_child_prefix}{span_fields} -- {span_name} {wide_msg} {elapsed_subsec}{color_end}",
        )
        .unwrap()
        .with_key(
            "elapsed_subsec",
            |state: &ProgressState, writer: &mut dyn std::fmt::Write| {
                let seconds = state.elapsed().as_secs();
                let sub_seconds = (state.elapsed().as_millis() % 1000) / 100;
                let _ = writer.write_str(&format!("{}.{}s", seconds, sub_seconds));
            },
        )
        .with_key(
            "color_start",
            |state: &ProgressState, writer: &mut dyn std::fmt::Write| {
                let elapsed = state.elapsed();

                if elapsed > Duration::from_secs(8) {
                    // Red
                    let _ = write!(writer, "\x1b[{}m", 1 + 30);
                } else if elapsed > Duration::from_secs(4) {
                    // Yellow
                    let _ = write!(writer, "\x1b[{}m", 3 + 30);
                }
            },
        )
        .with_key(
            "color_end",
            |state: &ProgressState, writer: &mut dyn std::fmt::Write| {
                if state.elapsed() > Duration::from_secs(4) {
                    let _ =write!(writer, "\x1b[0m");
                }
            },
        ),
    ).with_span_child_prefix_symbol("↳ ").with_span_child_prefix_indent(" ");

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_fmt_writer()))
        .with(indicatif_layer)
        .init();

    stream::iter((0..20).map(|val| build(val)))
        .buffer_unordered(7)
        .collect::<Vec<()>>()
        .await;
}
