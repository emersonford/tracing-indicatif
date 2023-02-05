use std::io;
use std::marker::PhantomData;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar};
use tracing_core::Subscriber;
use tracing_core::span;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer;
use tracing_subscriber::registry::LookupSpan;

// TODO(emersonford): find a cleaner way to integrate this layer with fmt::Layer.
#[derive(Clone)]
pub struct IndicatifWriter {
    progress_bars: MultiProgress,
}

impl io::Write for IndicatifWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress_bars.suspend(|| io::stderr().write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stderr().flush())
    }
}

impl<'a> MakeWriter<'a> for IndicatifWriter {
    type Writer = IndicatifWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

pub struct IndicatifLayer<S> {
    progress_bars: MultiProgress,
    inner: PhantomData<S>,
}

impl<S> IndicatifLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    pub fn new() -> Self {
        Self {
            progress_bars: MultiProgress::new(),
            inner: PhantomData,
        }
    }

    /// Returns the writer that should be passed into
    /// [fmt::Layer::with_writer](tracing_subscriber::fmt::Layer::with_writer).
    ///
    /// This will result in all log messages being written to stderr and ensures the printing of
    /// the log messages does not interfere with the progress bars.
    pub fn get_writer(&self) -> impl for<'writer> MakeWriter<'writer> {
        IndicatifWriter {
            // `MultiProgress` is merely a wrapper over an `Arc`, so we can clone here.
            progress_bars: self.progress_bars.clone(),
        }
    }
}

struct IndicatifSpanContext {
    progress_bar: Option<ProgressBar>,
}

impl<S> layer::Layer<S> for IndicatifLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        _attrs: &span::Attributes<'_>,
        id: &span::Id,
        ctx: layer::Context<'_, S>,
    ) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");
        let mut ext = span.extensions_mut();

        ext.insert(IndicatifSpanContext { progress_bar: None });
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");
        let mut ext = span.extensions_mut();

        if let Some(indicatif_ctx) = ext.get_mut::<IndicatifSpanContext>() {
            // Start the progress bar when we enter the span for the first time.
            indicatif_ctx.progress_bar.get_or_insert_with(|| {
                let pb = self.progress_bars.add(ProgressBar::new_spinner());
                pb.set_message(span.name());
                pb.enable_steady_tick(Duration::from_millis(50));

                pb
            });
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(&id)
            .expect("Span not found in context, this is a bug");
        let mut ext = span.extensions_mut();

        // Clear the progress bar only when the span has closed completely.
        if let Some(pb) = ext
            .get_mut::<IndicatifSpanContext>()
            .and_then(|indicatif_ctx| indicatif_ctx.progress_bar.take())
        {
            pb.finish_and_clear();

            self.progress_bars.remove(&pb);
        }
    }
}
