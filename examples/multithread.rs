use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() {
    let indicatif_layer = IndicatifLayer::new();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();

    let handles = (0..256)
        .map(|i| {
            std::thread::spawn(move || {
                tracing::info!("before {i}");
                let _span = tracing::info_span!("ch").entered();
                tracing::info!("after {i}");
            })
        })
        .collect::<Vec<_>>();

    handles.into_iter().for_each(|i| i.join().unwrap());
}
