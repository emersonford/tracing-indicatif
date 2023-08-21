use std::io::Write;
use std::time::Duration;

use dialoguer::Confirm;
use tracing::info_span;
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::writer::get_indicatif_stderr_writer;
use tracing_indicatif::{suspend_tracing_indicatif, IndicatifLayer};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() {
    let indicatif_layer = IndicatifLayer::new();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();

    let _span = info_span!("foo");
    _span.pb_start();

    std::thread::sleep(Duration::from_secs(1));

    suspend_tracing_indicatif(|| {
        if Confirm::new()
            .with_prompt("Do you like Rust?")
            .interact()
            .unwrap_or(false)
        {
            println!("Yay!");
        } else {
            println!("oh... okay :(");
        }
    });

    let _ = writeln!(
        get_indicatif_stderr_writer().unwrap(),
        "sleeping for some time..."
    );

    std::thread::sleep(Duration::from_secs(1));
}
