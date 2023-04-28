use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() {
    let indicatif_layer = IndicatifLayer::new();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();

    let handles = (0..32)
        .map(|_| {
            std::thread::spawn(move || {
                let _span = tracing::info_span!("ch").entered();

                let subhandles = (0..16)
                    .map(|_| {
                        std::thread::spawn(move || {
                            let _span = tracing::info_span!("subch").entered();
                        })
                    })
                    .collect::<Vec<_>>();

                subhandles.into_iter().for_each(|i| i.join().unwrap());
            })
        })
        .collect::<Vec<_>>();
    handles.into_iter().for_each(|i| i.join().unwrap());
}
