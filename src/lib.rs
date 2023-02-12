use std::collections::VecDeque;
use std::io;
use std::marker::PhantomData;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use indicatif::style::ProgressStyle;
use indicatif::style::ProgressTracker;
use indicatif::ProgressDrawTarget;
use indicatif::ProgressState;
use indicatif::{MultiProgress, ProgressBar};
use tracing_core::span;
use tracing_core::Subscriber;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::registry::SpanRef;

// TODO(emersonford): add support for incrementing/non-spinner progress bars (maybe with
// Span::current().increment_progress_bar() style API? or maybe by spinning one of the span's field
// to the progress increment?)
// TODO(emersonford): allow specifying progress bar style per span
// TODO(emersonford): update format field in span's `on_record`.
// TODO(emersonford): expose an stdout writer

#[derive(Clone)]
struct IndicatifProgressKey {
    message: String,
}

impl ProgressTracker for IndicatifProgressKey {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(self.clone())
    }

    fn tick(&mut self, _: &indicatif::ProgressState, _: std::time::Instant) {}

    fn reset(&mut self, _: &indicatif::ProgressState, _: std::time::Instant) {}

    fn write(&self, _: &indicatif::ProgressState, w: &mut dyn std::fmt::Write) {
        let _ = w.write_str(&self.message);
    }
}

// TODO(emersonford): find a cleaner way to integrate this layer with fmt::Layer.
/// A wrapper around [std::io::stderr()] that ensures log entries from tracing's fmt layer are
/// printed above any active progress bars to ensure those log entries are not clobbered by
/// active progress bars.
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

struct IndicatifSpanContext {
    // If this progress bar is `Some(pb)` and `pb.is_hidden`, it means the progress bar is queued.
    // We start the progress bar in hidden mode so things like `elapsed` are accurate.
    //
    // If this progress bar is `None`, it means the span has not yet been entered.
    progress_bar: Option<ProgressBar>,
    // Notes:
    // * A parent span cannot close before its child spans, so if a parent span has a progress bar,
    //   that parent progress bar's lifetime will be greater than this span's progress bar.
    // * The ProgressBar is just a wrapper around `Arc`, so cloning and tracking it here is fine.
    parent_progress_bar: Option<ProgressBar>,
    span_fields_formatted: Option<String>,
    // Used to quickly compute a child span's prefix without having to traverse up the entire span
    // scope.
    level: u16,
}

struct ProgressBarManager {
    mp: MultiProgress,
    active_progress_bars: u64,
    max_progress_bars: u64,
    // This is used in the footer progress bar and tracks the actual number of pending progress
    // bars.
    pending_progress_bars: Arc<AtomicU64>,
    // The `.len()` of this may differ from `pending_progress_bars`. If a span closes before its
    // progress bar is ever un-hidden, we decrement `pending_progress_bars` but won't clean the
    // span entry up from this `VecDeque` for performance reasons. Instead, whenever we do un-hide
    // a progress bar, we'll "garbage collect" closed spans from this then.
    pending_spans: VecDeque<span::Id>,
    // If this is `None`, a footer will never be shown.
    footer_pb: Option<ProgressBar>,
}

impl ProgressBarManager {
    fn new(max_progress_bars: u64, footer_progress_style: Option<ProgressStyle>) -> Self {
        let pending_progress_bars = Arc::new(AtomicU64::new(0));

        Self {
            mp: {
                let mp = MultiProgress::new();
                mp.set_draw_target(ProgressDrawTarget::stderr_with_hz(20));

                mp
            },
            active_progress_bars: 0,
            max_progress_bars,
            pending_progress_bars: pending_progress_bars.clone(),
            pending_spans: VecDeque::new(),
            footer_pb: footer_progress_style.map(|style| {
                ProgressBar::hidden().with_style(style.with_key(
                    "pending_progress_bars",
                    move |_: &ProgressState, writer: &mut dyn std::fmt::Write| {
                        let _ = write!(
                            writer,
                            "{}",
                            pending_progress_bars.load(std::sync::atomic::Ordering::SeqCst)
                        );
                    },
                ))
            }),
        }
    }

    fn decrement_pending_pb(&mut self) {
        let prev_val = self
            .pending_progress_bars
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

        // If this span was the last one pending, clear the footer (if it was active).
        if prev_val == 1 {
            debug_assert!(
                self.footer_pb
                    .as_ref()
                    .map(|pb| !pb.is_hidden())
                    .unwrap_or(true),
                "footer progress bar was hidden despite there being pending progress bars"
            );

            if let Some(footer_pb) = self.footer_pb.as_ref() {
                footer_pb.finish_and_clear();
                self.mp.remove(footer_pb);
                footer_pb.disable_steady_tick();
                self.mp.set_move_cursor(false);
            }
        }
    }

    fn add_pending_pb(&mut self, span_id: &span::Id) {
        let prev_val = self
            .pending_progress_bars
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.pending_spans.push_back(span_id.clone());

        if prev_val == 0 {
            debug_assert!(
                self.footer_pb
                    .as_ref()
                    .map(|pb| pb.is_hidden())
                    .unwrap_or(true),
                "footer progress bar was not hidden despite there being no pending progress bars"
            );

            // Show the footer progress bar.
            if let Some(footer_pb) = self.footer_pb.take() {
                let pb = self.mp.add(footer_pb);
                pb.enable_steady_tick(Duration::from_millis(100));
                self.mp.set_move_cursor(true);

                self.footer_pb = Some(pb);
            }
        }
    }

    fn show_progress_bar(&mut self, pb_span_ctx: &mut IndicatifSpanContext, span_id: &span::Id) {
        if self.active_progress_bars < self.max_progress_bars {
            let pb = match pb_span_ctx.parent_progress_bar {
                // TODO(emersonford): fix span ordering in progress bar, because we use
                // `insert_after`, we end up showing the child progress bars in reverse order.
                Some(ref parent_pb) => self
                    .mp
                    .insert_after(parent_pb, pb_span_ctx.progress_bar.take().unwrap()),
                None => {
                    if self
                        .footer_pb
                        .as_ref()
                        .map(|footer_pb| !footer_pb.is_hidden())
                        .unwrap_or(false)
                    {
                        self.mp
                            .insert_from_back(1, pb_span_ctx.progress_bar.take().unwrap())
                    } else {
                        self.mp.add(pb_span_ctx.progress_bar.take().unwrap())
                    }
                }
            };

            self.active_progress_bars += 1;

            pb.enable_steady_tick(Duration::from_millis(100));
            pb_span_ctx.progress_bar = Some(pb);
        } else {
            self.add_pending_pb(span_id);
        }
    }

    fn finish_progress_bar<S>(
        &mut self,
        pb_span_ctx: &mut IndicatifSpanContext,
        ctx: &layer::Context<'_, S>,
    ) where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let Some(pb) = pb_span_ctx.progress_bar.take() else {
            // Span was never entered.
            return;
        };

        // The span closed before we had a chance to show its progress bar.
        if pb.is_hidden() {
            self.decrement_pending_pb();
            return;
        }

        // This span had an active/shown progress bar.
        pb.finish_and_clear();
        self.mp.remove(&pb);
        self.active_progress_bars -= 1;

        let maybe_next_eligible_span: Option<(span::Id, SpanRef<S>)> = loop {
            let Some(span_id) = self.pending_spans.pop_front() else {
                break None;
            };

            match ctx.span(&span_id) {
                Some(v) => {
                    break Some((span_id, v));
                }
                None => {
                    // Span was closed earlier, we "garbage collect" it from the queue here.
                    continue;
                }
            }
        };

        let Some((span_id, next_eligible_span)) = maybe_next_eligible_span else {
            return;
        };

        let mut ext = next_eligible_span.extensions_mut();
        let indicatif_span_ctx = ext
            .get_mut::<IndicatifSpanContext>()
            .expect("No IndicatifSpanContext found; this is a bug");

        self.decrement_pending_pb();
        self.show_progress_bar(indicatif_span_ctx, &span_id);
    }
}

/// The layer that handles creating and managing indicatif progress bars for active spans. This
/// layer must be registered with your tracing subscriber to have any effect.
///
/// Under the hood, this just uses indicatif's [MultiProgress] struct to manage individual
/// [ProgressBar] instances per span.
///
/// This layer performs no filtering on which spans to show progress bars for. It is expected one
/// attaches [filters to this
/// layer](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/index.html#filtering-with-layers)
/// to control which spans actually have progress bars generated for them.
pub struct IndicatifLayer<S, F = DefaultFields> {
    pb_manager: Mutex<ProgressBarManager>,
    span_field_formatter: F,
    progress_style: ProgressStyle,
    span_child_prefix_indent: &'static str,
    span_child_prefix_symbol: &'static str,
    inner: PhantomData<S>,
}

impl<S> IndicatifLayer<S> {
    /// Spawns a progress bar for every tracing span that is received by this layer.
    ///
    /// The default settings for this layer are 7 progress bars maximum and progress bars in the
    /// style of:
    /// ```text
    /// ⠄ do_work{val=0}
    /// ⠄ do_work{val=1}
    /// ⠄ do_work{val=2}
    ///   ↳ ⠴ do_sub_work{val=2}
    ///   ↳ ⠴ do_sub_work{val=2}
    /// ⠄ do_work{val=3}
    /// ⠄ do_work{val=4}
    /// ...and 5 more not shown above.
    /// ```
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S> Default for IndicatifLayer<S> {
    fn default() -> Self {
        Self {
            pb_manager: Mutex::new(ProgressBarManager::new(
                7,
                Some(
                    ProgressStyle::with_template(
                        "...and {pending_progress_bars} more not shown above.",
                    )
                    .unwrap(),
                ),
            )),
            span_field_formatter: DefaultFields::new(),
            progress_style: ProgressStyle::with_template(
                "{span_child_prefix}{spinner} {span_name}{{{span_fields}}}",
            )
            .unwrap(),
            span_child_prefix_indent: "  ",
            span_child_prefix_symbol: "↳ ",
            inner: PhantomData,
        }
    }
}

// pub methods
impl<S, F> IndicatifLayer<S, F> {
    /// Returns the writer that should be passed into
    /// [fmt::Layer::with_writer](tracing_subscriber::fmt::Layer::with_writer).
    ///
    /// This will result in all log messages being written to stderr and ensures the printing of
    /// the log messages does not interfere with the progress bars.
    pub fn get_fmt_writer(&self) -> IndicatifWriter {
        IndicatifWriter {
            // `MultiProgress` is merely a wrapper over an `Arc`, so we can clone here.
            progress_bars: self.pb_manager.lock().unwrap().mp.clone(),
        }
    }

    /// Set the formatter for span fields, the result of which will be available as the
    /// progress bar template key `span_fields`.
    ///
    /// The default is the [DefaultFields] formatter.
    pub fn with_span_field_formatter<F2>(self, formatter: F2) -> IndicatifLayer<S, F2>
    where
        F2: for<'writer> FormatFields<'writer> + 'static,
    {
        IndicatifLayer {
            pb_manager: self.pb_manager,
            span_field_formatter: formatter,
            progress_style: self.progress_style,
            span_child_prefix_indent: self.span_child_prefix_indent,
            span_child_prefix_symbol: self.span_child_prefix_symbol,
            inner: self.inner,
        }
    }

    /// Override the style used for displayed progress bars.
    ///
    /// Two additional keys are available for the progress bar template:
    /// * `span_fields` - the formatted string of this span's fields
    /// * `span_name` - the name of the span
    /// * `span_child_prefix` - a prefix that increase in size according to the number of parents
    ///   the span has.
    ///
    /// The default template is `{span_child_prefix}{spinner} {span_name}{{{span_fields}}}`.
    pub fn with_progress_style(mut self, style: ProgressStyle) -> Self {
        self.progress_style = style;
        self
    }

    /// Set the indent used to mark the "level" of a given child span's progress bar.
    ///
    /// For example, if the given span is two levels deep (iow has two parent spans with progress
    /// bars), and this is " ", the `{span_child_prefix}` key for this span's progress bar will be
    /// prefixed with "  ".
    pub fn with_span_child_prefix_indent(mut self, indent: &'static str) -> Self {
        self.span_child_prefix_indent = indent;
        self
    }

    /// Set the symbol used to denote this is a progress bar from a child span.
    ///
    /// This is ultimately concatenated with the child prefix indent to make the
    /// `span_child_prefix` progress bar key.
    pub fn with_span_child_prefix_symbol(mut self, symbol: &'static str) -> Self {
        self.span_child_prefix_symbol = symbol;
        self
    }

    /// Set the maximum number of progress bars that will be displayed, and the possible footer
    /// "progress bar" that displays when there are more progress bars than can be displayed.
    ///
    /// `footer_style` dictates the appearance of the footer, and the footer will only appear if
    /// there are more progress bars than can be displayed. If it is `None`, no footer will be
    /// displayed. `footer_style` has the following keys available to it:
    /// * `pending_progress_bars` - the number of progress bars waiting to be shown
    pub fn with_max_progress_bars(
        mut self,
        max_progress_bars: u64,
        footer_style: Option<ProgressStyle>,
    ) -> Self {
        self.pb_manager = Mutex::new(ProgressBarManager::new(max_progress_bars, footer_style));
        self
    }
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
        let _ = self
            .span_field_formatter
            .format_fields(fields.as_writer(), attrs);

        ext.insert(IndicatifSpanContext {
            progress_bar: None,
            parent_progress_bar: None,
            span_fields_formatted: Some(fields.fields),
            level: 0,
        });
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");
        let mut ext = span.extensions_mut();

        if let Some(indicatif_ctx) = ext.get_mut::<IndicatifSpanContext>() {
            // Get the next parent span with a progress bar.
            let parent_span = ctx.span_scope(id).and_then(|scope| {
                scope.skip(1).find(|span| {
                    let ext = span.extensions();

                    ext.get::<IndicatifSpanContext>().is_some()
                })
            });
            let parent_span_ext = parent_span.as_ref().map(|span| span.extensions());
            let parent_indicatif_ctx = parent_span_ext
                .as_ref()
                .map(|ext| ext.get::<IndicatifSpanContext>().unwrap());

            let span_name = span.name().to_string();
            let span_fields_formatted = indicatif_ctx
                .span_fields_formatted
                .to_owned()
                .unwrap_or_default();

            // Start the progress bar when we enter the span for the first time.
            if indicatif_ctx.progress_bar.is_none() {
                let span_child_prefix = match parent_indicatif_ctx {
                    Some(v) => {
                        indicatif_ctx.level = v.level + 1;

                        format!(
                            "{}{}",
                            self.span_child_prefix_indent
                                .repeat(indicatif_ctx.level.into()),
                            self.span_child_prefix_symbol
                        )
                    }
                    None => String::new(),
                };

                indicatif_ctx.progress_bar = Some(
                    ProgressBar::hidden().with_style(
                        self.progress_style
                            .clone()
                            .with_key("span_name", IndicatifProgressKey { message: span_name })
                            .with_key(
                                "span_fields",
                                IndicatifProgressKey {
                                    message: span_fields_formatted,
                                },
                            )
                            .with_key(
                                "span_child_prefix",
                                IndicatifProgressKey {
                                    message: span_child_prefix,
                                },
                            ),
                    ),
                );

                if let Some(parent_indicatif_ctx) = parent_indicatif_ctx {
                    // Parent spans should always have been entered at least once, meaning if
                    // they have an `IndicatifSpanContext`, they have a progress bar. So we can
                    // unwrap safely here.
                    //
                    // TODO(emersonford): is this actually true? :o
                    indicatif_ctx.parent_progress_bar =
                        Some(parent_indicatif_ctx.progress_bar.to_owned().unwrap());
                }

                self.pb_manager
                    .lock()
                    .unwrap()
                    .show_progress_bar(indicatif_ctx, id);
            }
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx
            .span(&id)
            .expect("Span not found in context, this is a bug");
        let mut ext = span.extensions_mut();

        // Clear the progress bar only when the span has closed completely.
        if let Some(indicatif_ctx) = ext.get_mut::<IndicatifSpanContext>() {
            self.pb_manager
                .lock()
                .unwrap()
                .finish_progress_bar(indicatif_ctx, &ctx);
        }
    }
}
