use std::io;
use std::marker::PhantomData;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar};
use tracing_core::span;
use tracing_core::Subscriber;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::FormattedFields;
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

pub struct IndicatifLayer<S, F = DefaultFields> {
    progress_bars: MultiProgress,
    field_formatter: F,
    inner: PhantomData<S>,
}

impl<S> IndicatifLayer<S> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S> Default for IndicatifLayer<S> {
    fn default() -> Self {
        Self {
            progress_bars: MultiProgress::new(),
            field_formatter: DefaultFields::new(),
            inner: PhantomData,
        }
    }
}

impl<S, F> IndicatifLayer<S, F> {
    pub fn fmt_fields<F2>(self, fmt_fields: F2) -> IndicatifLayer<S, F2>
    where
        F2: for<'writer> FormatFields<'writer> + 'static,
    {
        IndicatifLayer {
            progress_bars: self.progress_bars,
            field_formatter: fmt_fields,
            inner: self.inner,
        }
    }

    /// Returns the writer that should be passed into
    /// [fmt::Layer::with_writer](tracing_subscriber::fmt::Layer::with_writer).
    ///
    /// This will result in all log messages being written to stderr and ensures the printing of
    /// the log messages does not interfere with the progress bars.
    pub fn get_writer(&self) -> IndicatifWriter {
        IndicatifWriter {
            // `MultiProgress` is merely a wrapper over an `Arc`, so we can clone here.
            progress_bars: self.progress_bars.clone(),
        }
    }
}

struct IndicatifSpanContext {
    progress_bar: Option<ProgressBar>,
    message: Option<String>,
}

impl<S, F> layer::Layer<S> for IndicatifLayer<S, F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'writer> FormatFields<'writer> + 'static,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");
        let mut ext = span.extensions_mut();

        let mut fields = FormattedFields::<F>::new(String::new());
        let _ = self.field_formatter
            .format_fields(fields.as_writer(), attrs);

        ext.insert(IndicatifSpanContext {
            progress_bar: None,
            message: Some(format!("{}{{{}}}", span.name(), fields.fields)),
        });
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
                pb.set_message(
                    indicatif_ctx
                        .message
                        .as_deref()
                        .unwrap_or_else(|| span.name())
                        .to_string(),
                );
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
