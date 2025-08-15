//! A [tracing](https://docs.rs/tracing/latest/tracing/) layer that automatically creates and manages [indicatif](https://docs.rs/indicatif/latest/indicatif/index.html) progress bars for active spans.
//!
//! Progress bars are a great way to make your CLIs feel more responsive. However,
//! adding and managing progress bars in your libraries can be invasive, unergonomic,
//! and difficult to keep track of.
//!
//! This library aims to make it easy to show progress bars for your CLI by tying
//! progress bars to [tracing spans](https://docs.rs/tracing/latest/tracing/#spans).
//! For CLIs/libraries already using tracing spans, this allow for a dead simple (3
//! line) code change to enable a smooth progress bar experience for your program.
//! This eliminates having to have code in your libraries to manually manage
//! progress bar instances.
//!
//! This ends up working quite well as progress bars are fundamentally tracking the
//! lifetime of some "span" (whether that "span" is defined explicitly or implicitly),
//! so might as well make that relationship explicit.
//!
//! An easy quick start for this crate is:
//! ```rust
//! use tracing_indicatif::IndicatifLayer;
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_subscriber::util::SubscriberInitExt;
//!
//! let indicatif_layer = IndicatifLayer::new();
//!
//! tracing_subscriber::registry()
//!     .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
//!     .with(indicatif_layer)
//!     .init();
//! ```
//! See [`IndicatifLayer`] for additional documentation.
//!
//! See the [`examples`](https://github.com/emersonford/tracing-indicatif/tree/main/examples) folder for examples of how to customize the layer / progress bar
//! appearance.
//!
//! Note: it is highly recommended you pass `indicatif_layer.get_stderr_writer()` or
//! `indicatif_layer.get_stdout_writer()` to your `fmt::layer()` (depending on where you want to
//! emit tracing logs) to prevent progress bars from clobbering any console logs.
use std::any::TypeId;
use std::marker::PhantomData;
use std::sync::Mutex;

use indicatif::MultiProgress;
use indicatif::ProgressBar;
/// Re-export of [`indicatif`]'s style module for ease of use.
pub use indicatif::style;
use indicatif::style::ProgressStyle;
use indicatif::style::ProgressTracker;
use tracing_core::Subscriber;
use tracing_core::span;
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::layer;
use tracing_subscriber::registry::LookupSpan;

pub mod filter;
mod pb_manager;
pub mod span_ext;
pub mod util;
pub mod writer;

use pb_manager::ProgressBarManager;
pub use pb_manager::TickSettings;
#[doc(inline)]
pub use writer::IndicatifWriter;

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

// Suppose we have a [Span] (maybe gotten via [Span::current]) and want access to our
// [IndicatifLayer] instance from it. The way to do this would be something like
// ```
// span.with_subscriber(|(id, subscriber)| {
//   let maybe_layer = subscriber.downcast_ref::<IndicatifLayer<S, F>>();
//   ...
// });
// ```
// but this has the problem that, because `IndicatifLayer` has generic params, we need to pass
// a concrete type `S` and `F` to that `downcast_ref` call. And the callsite doesn't know what
// those concrete types are.
//
// Therefore, we use this `WithContext` struct (along with the defined `downcast_raw` method) to do
// a form of indirection to something that does already know (or "remembers") what those concrete
// types `S` and `F` are, so the callsite doesn't need to care about it.
//
// This doesn't actually return a reference to our [IndicatifLayer] instance as we only care about
// the associated span data, so we just pass that to the corresponding `fn`.
//
// See:
// * https://github.com/tokio-rs/tracing/blob/a0126b2e2d465e8e6d514acdf128fcef5b863d27/tracing-error/src/subscriber.rs#L32
// * https://github.com/tokio-rs/tracing/blob/a0126b2e2d465e8e6d514acdf128fcef5b863d27/tracing-opentelemetry/src/subscriber.rs#L74
#[allow(clippy::type_complexity)]
pub(crate) struct WithContext(
    fn(&tracing::Dispatch, &span::Id, f: &mut dyn FnMut(&mut IndicatifSpanContext)),
);

#[allow(clippy::type_complexity)]
pub(crate) struct WithStderrWriter(
    fn(&tracing::Dispatch, f: &mut dyn FnMut(IndicatifWriter<writer::Stderr>)),
);

#[allow(clippy::type_complexity)]
pub(crate) struct WithStdoutWriter(
    fn(&tracing::Dispatch, f: &mut dyn FnMut(IndicatifWriter<writer::Stdout>)),
);

#[allow(clippy::type_complexity)]
pub(crate) struct WithMultiProgress(fn(&tracing::Dispatch, f: &mut dyn FnMut(MultiProgress)));

impl WithContext {
    pub(crate) fn with_context(
        &self,
        dispatch: &tracing::Dispatch,
        id: &span::Id,
        mut f: impl FnMut(&mut IndicatifSpanContext),
    ) {
        (self.0)(dispatch, id, &mut f)
    }
}

impl WithStderrWriter {
    pub(crate) fn with_context(
        &self,
        dispatch: &tracing::Dispatch,
        mut f: impl FnMut(IndicatifWriter<writer::Stderr>),
    ) {
        (self.0)(dispatch, &mut f)
    }
}

impl WithStdoutWriter {
    pub(crate) fn with_context(
        &self,
        dispatch: &tracing::Dispatch,
        mut f: impl FnMut(IndicatifWriter<writer::Stdout>),
    ) {
        (self.0)(dispatch, &mut f)
    }
}

impl WithMultiProgress {
    pub(crate) fn with_context(
        &self,
        dispatch: &tracing::Dispatch,
        mut f: impl FnMut(MultiProgress),
    ) {
        (self.0)(dispatch, &mut f)
    }
}

#[derive(Default)]
struct ProgressBarInitSettings {
    style: Option<ProgressStyle>,
    len: Option<u64>,
    pos: Option<u64>,
    message: Option<String>,
}

struct IndicatifSpanContext {
    // If this progress bar is `Some(pb)` and `pb.is_hidden`, it means the progress bar is queued.
    // We start the progress bar in hidden mode so things like `elapsed` are accurate.
    //
    // If this progress bar is `None`, it means the span has not yet been entered.
    progress_bar: Option<ProgressBar>,
    // If `Some`, the progress bar will use this style when the span is entered for the first time.
    pb_init_settings: ProgressBarInitSettings,
    // Notes:
    // * A parent span cannot close before its child spans, so if a parent span has a progress bar,
    //   that parent progress bar's lifetime will be greater than this span's progress bar.
    // * The ProgressBar is just a wrapper around `Arc`, so cloning and tracking it here is fine.
    parent_progress_bar: Option<ProgressBar>,
    // This is only `Some` if we have some parent with a progress bar.
    parent_span: Option<span::Id>,
    // Fields to be passed to the progress bar as keys.
    span_fields_formatted: Option<String>,
    span_name: String,
    span_child_prefix: String,
    // Used to quickly compute a child span's prefix without having to traverse up the entire span
    // scope.
    level: u16,
    // If `Some`, this is the message that will be displayed when the progress bar is finished.
    finish_message: Option<String>,
}

impl IndicatifSpanContext {
    fn add_keys_to_style(&self, style: ProgressStyle) -> ProgressStyle {
        style
            .with_key(
                "span_name",
                IndicatifProgressKey {
                    message: self.span_name.clone(),
                },
            )
            .with_key(
                "span_fields",
                IndicatifProgressKey {
                    message: self.span_fields_formatted.to_owned().unwrap_or_default(),
                },
            )
            .with_key(
                "span_child_prefix",
                IndicatifProgressKey {
                    message: self.span_child_prefix.clone(),
                },
            )
    }

    fn make_progress_bar(&mut self, default_style: &ProgressStyle) {
        if self.progress_bar.is_none() {
            let pb = ProgressBar::hidden().with_style(
                self.pb_init_settings
                    .style
                    .take()
                    .unwrap_or_else(|| self.add_keys_to_style(default_style.clone())),
            );

            if let Some(len) = self.pb_init_settings.len.take() {
                pb.set_length(len);
            }

            if let Some(msg) = self.pb_init_settings.message.take() {
                pb.set_message(msg);
            }

            if let Some(pos) = self.pb_init_settings.pos.take() {
                pb.set_position(pos);
            }

            self.progress_bar = Some(pb);
        }
    }

    fn set_progress_bar_style(&mut self, style: ProgressStyle) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_style(self.add_keys_to_style(style));
        } else {
            self.pb_init_settings.style = Some(self.add_keys_to_style(style));
        }
    }

    fn set_progress_bar_length(&mut self, len: u64) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_length(len);
        } else {
            self.pb_init_settings.len = Some(len);
        }
    }

    fn set_progress_bar_position(&mut self, pos: u64) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_position(pos);
        } else {
            self.pb_init_settings.pos = Some(pos);
        }
    }

    fn set_progress_bar_message(&mut self, msg: String) {
        if let Some(ref pb) = self.progress_bar {
            pb.set_message(msg);
        } else {
            self.pb_init_settings.message = Some(msg);
        }
    }

    fn inc_progress_bar_position(&mut self, pos: u64) {
        if let Some(ref pb) = self.progress_bar {
            pb.inc(pos);
        } else if let Some(ref mut pb_pos) = self.pb_init_settings.pos {
            *pb_pos += pos;
        } else {
            // indicatif defaults position to 0, so copy that behavior.
            self.pb_init_settings.pos = Some(pos);
        }
    }

    fn inc_progress_bar_length(&mut self, len: u64) {
        if let Some(ref pb) = self.progress_bar {
            pb.inc_length(len);
        } else if let Some(ref mut pb_len) = self.pb_init_settings.len {
            *pb_len += len;
        }
    }

    fn progress_bar_tick(&mut self) {
        if let Some(ref pb) = self.progress_bar {
            pb.tick()
        }
    }

    fn reset_progress_bar(&mut self) {
        if let Some(ref pb) = self.progress_bar {
            pb.reset();
        }
    }

    fn reset_progress_bar_elapsed(&mut self) {
        if let Some(ref pb) = self.progress_bar {
            pb.reset_elapsed();
        }
    }

    fn reset_progress_bar_eta(&mut self) {
        if let Some(ref pb) = self.progress_bar {
            pb.reset_eta();
        }
    }

    fn set_progress_bar_finish_message(&mut self, msg: String) {
        self.finish_message = Some(msg);
    }
}

/// The layer that handles creating and managing indicatif progress bars for active spans. This
/// layer must be registered with your tracing subscriber to have any effect.
///
/// This layer performs no filtering on which spans to show progress bars for. It is expected one
/// attaches [filters to this
/// layer](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/index.html#filtering-with-layers)
/// to control which spans actually have progress bars generated for them. See
/// [`filter::IndicatifFilter`] for a rudimentary filter.
///
/// Progress bars will be started the very first time a span is [entered](tracing::Span::enter)
/// or when one of its child spans is entered for the first time, and will finish when the span
/// is [closed](tracing_subscriber::Layer::on_close) (including all child spans having closed).
///
/// Progress bars are emitted to stderr.
///
/// Under the hood, this just uses indicatif's [`MultiProgress`] struct to
/// manage individual [`ProgressBar`] instances per span.
pub struct IndicatifLayer<S, F = DefaultFields> {
    pb_manager: Mutex<ProgressBarManager>,
    // Allows us to fetch the `MultiProgress` without taking a lock.
    // Do not mutate `mp` directly, always go through `pb_manager`.
    mp: MultiProgress,
    span_field_formatter: F,
    progress_style: ProgressStyle,
    span_child_prefix_indent: &'static str,
    span_child_prefix_symbol: &'static str,
    get_context: WithContext,
    get_stderr_writer_context: WithStderrWriter,
    get_stdout_writer_context: WithStdoutWriter,
    get_multi_progress_context: WithMultiProgress,
    inner: PhantomData<S>,
}

impl<S> IndicatifLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
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

impl<S> Default for IndicatifLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn default() -> Self {
        let pb_manager = ProgressBarManager::new(
            7,
            Some(
                ProgressStyle::with_template(
                    "...and {pending_progress_bars} more not shown above.",
                )
                .expect("valid template"),
            ),
            TickSettings::default(),
        );
        let mp = pb_manager.mp.clone();

        Self {
            pb_manager: Mutex::new(pb_manager),
            mp,
            span_field_formatter: DefaultFields::new(),
            progress_style: ProgressStyle::with_template(
                "{span_child_prefix}{spinner} {span_name}{{{span_fields}}}",
            )
            .expect("valid template"),
            span_child_prefix_indent: "  ",
            span_child_prefix_symbol: "↳ ",
            get_context: WithContext(Self::get_context),
            get_stderr_writer_context: WithStderrWriter(Self::get_stderr_writer_context),
            get_stdout_writer_context: WithStdoutWriter(Self::get_stdout_writer_context),
            get_multi_progress_context: WithMultiProgress(Self::get_multi_progress_context),
            inner: PhantomData,
        }
    }
}

// pub methods
impl<S, F> IndicatifLayer<S, F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    #[deprecated(since = "0.2.3", note = "use get_stderr_writer() instead")]
    pub fn get_fmt_writer(&self) -> IndicatifWriter<writer::Stderr> {
        self.get_stderr_writer()
    }

    /// Returns the a writer for [`std::io::Stderr`] that ensures its output will not be clobbered by
    /// active progress bars.
    ///
    /// Instead of `eprintln!(...)` prefer `writeln!(indicatif_layer.get_stderr_writer(), ...)`
    /// instead to ensure your output is not clobbered by active progress bars.
    ///
    /// If one wishes tracing logs to be output to stderr, this should be passed into
    /// [`fmt::Layer::with_writer`](tracing_subscriber::fmt::Layer::with_writer).
    pub fn get_stderr_writer(&self) -> IndicatifWriter<writer::Stderr> {
        // `MultiProgress` is merely a wrapper over an `Arc`, so we can clone here.
        IndicatifWriter::new(self.mp.clone())
    }

    /// Returns the a writer for [`std::io::Stdout`] that ensures its output will not be clobbered by
    /// active progress bars.
    ///
    /// Instead of `println!(...)` prefer `writeln!(indicatif_layer.get_stdout_writer(), ...)`
    /// instead to ensure your output is not clobbered by active progress bars.
    ///
    /// If one wishes tracing logs to be output to stdout, this should be passed into
    /// [`fmt::Layer::with_writer`](tracing_subscriber::fmt::Layer::with_writer).
    pub fn get_stdout_writer(&self) -> IndicatifWriter<writer::Stdout> {
        // `MultiProgress` is merely a wrapper over an `Arc`, so we can clone here.
        IndicatifWriter::new(self.mp.clone())
    }

    /// Set the formatter for span fields, the result of which will be available as the
    /// progress bar template key `span_fields`.
    ///
    /// The default is the [`DefaultFields`] formatter.
    pub fn with_span_field_formatter<F2>(self, formatter: F2) -> IndicatifLayer<S, F2>
    where
        F2: for<'writer> FormatFields<'writer> + 'static,
    {
        IndicatifLayer {
            pb_manager: self.pb_manager,
            mp: self.mp,
            span_field_formatter: formatter,
            progress_style: self.progress_style,
            span_child_prefix_indent: self.span_child_prefix_indent,
            span_child_prefix_symbol: self.span_child_prefix_symbol,
            get_context: WithContext(IndicatifLayer::<S, F2>::get_context),
            get_stderr_writer_context: WithStderrWriter(
                IndicatifLayer::<S, F2>::get_stderr_writer_context,
            ),
            get_stdout_writer_context: WithStdoutWriter(
                IndicatifLayer::<S, F2>::get_stdout_writer_context,
            ),
            get_multi_progress_context: WithMultiProgress(
                IndicatifLayer::<S, F2>::get_multi_progress_context,
            ),
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
        if let Ok(pb_manager) = self.pb_manager.get_mut() {
            pb_manager.set_max_progress_bars(max_progress_bars, footer_style);
        }

        self
    }

    /// Configures how often progress bars are recalcuated and redrawn to the terminal.
    pub fn with_tick_settings(mut self, tick_settings: TickSettings) -> Self {
        if let Ok(pb_manager) = self.pb_manager.get_mut() {
            pb_manager.set_tick_settings(tick_settings);
        }

        self
    }
}

impl<S, F> IndicatifLayer<S, F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'writer> FormatFields<'writer> + 'static,
{
    fn get_context(
        dispatch: &tracing::Dispatch,
        id: &span::Id,
        f: &mut dyn FnMut(&mut IndicatifSpanContext),
    ) {
        // The only way `get_context` can be called is if we have an `IndicatifLayer` added to the
        // expected subscriber, hence why we can `.expect` here.
        let subscriber = dispatch
            .downcast_ref::<S>()
            .expect("subscriber should downcast to expected type; this is a bug!");
        let span = subscriber
            .span(id)
            .expect("Span not found in context, this is a bug");

        let mut ext = span.extensions_mut();

        if let Some(indicatif_ctx) = ext.get_mut::<IndicatifSpanContext>() {
            f(indicatif_ctx);
        }
    }

    fn get_stderr_writer_context(
        dispatch: &tracing::Dispatch,
        f: &mut dyn FnMut(IndicatifWriter<writer::Stderr>),
    ) {
        let layer = dispatch
            .downcast_ref::<IndicatifLayer<S, F>>()
            .expect("subscriber should downcast to expected type; this is a bug!");

        f(layer.get_stderr_writer())
    }

    fn get_stdout_writer_context(
        dispatch: &tracing::Dispatch,
        f: &mut dyn FnMut(IndicatifWriter<writer::Stdout>),
    ) {
        let layer = dispatch
            .downcast_ref::<IndicatifLayer<S, F>>()
            .expect("subscriber should downcast to expected type; this is a bug!");

        f(layer.get_stdout_writer())
    }

    fn get_multi_progress_context(dispatch: &tracing::Dispatch, f: &mut dyn FnMut(MultiProgress)) {
        let layer = dispatch
            .downcast_ref::<IndicatifLayer<S, F>>()
            .expect("subscriber should downcast to expected type; this is a bug!");

        f(layer.mp.clone())
    }

    fn handle_on_enter(
        &self,
        pb_manager: &mut ProgressBarManager,
        id: &span::Id,
        ctx: &layer::Context<'_, S>,
    ) -> Option<ProgressBar> {
        let span = ctx
            .span(id)
            .expect("Span not found in context, this is a bug");
        let mut ext = span.extensions_mut();

        if let Some(indicatif_ctx) = ext.get_mut::<IndicatifSpanContext>() {
            // Start the progress bar when we enter the span for the first time.
            if indicatif_ctx.progress_bar.is_none() {
                indicatif_ctx.make_progress_bar(&self.progress_style);

                if let Some(ref parent_span_with_pb) = indicatif_ctx.parent_span {
                    // Recursively start parent PBs if parent spans have not been entered yet.
                    let parent_pb = self.handle_on_enter(pb_manager, parent_span_with_pb, ctx);

                    indicatif_ctx.parent_progress_bar = parent_pb;
                }

                pb_manager.show_progress_bar(indicatif_ctx, id);
            }

            return indicatif_ctx.progress_bar.to_owned();
        }

        None
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

        // Get the next parent span with a progress bar.
        let parent_span = ctx.span_scope(id).and_then(|scope| {
            scope.skip(1).find(|span| {
                let ext = span.extensions();

                ext.get::<IndicatifSpanContext>().is_some()
            })
        });
        let parent_span_id = parent_span.as_ref().map(|span| span.id());
        let parent_span_ext = parent_span.as_ref().map(|span| span.extensions());
        let parent_indicatif_ctx = parent_span_ext.as_ref().map(|ext| {
            ext.get::<IndicatifSpanContext>()
                .expect("validated it exists prior")
        });

        let (span_child_prefix, level) = match parent_indicatif_ctx {
            Some(v) => {
                let level = v.level + 1;

                (
                    format!(
                        "{}{}",
                        self.span_child_prefix_indent.repeat(level.into()),
                        self.span_child_prefix_symbol
                    ),
                    level,
                )
            }
            None => (String::new(), 0),
        };

        ext.insert(IndicatifSpanContext {
            progress_bar: None,
            pb_init_settings: ProgressBarInitSettings::default(),
            parent_progress_bar: None,
            parent_span: parent_span_id,
            span_fields_formatted: Some(fields.fields),
            span_name: span.name().to_string(),
            span_child_prefix,
            level,
            finish_message: None,
        });
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        if let Ok(mut pb_manager_lock) = self.pb_manager.lock() {
            self.handle_on_enter(&mut pb_manager_lock, id, &ctx);
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        if let Ok(mut pb_manager_lock) = self.pb_manager.lock() {
            let span = ctx
                .span(&id)
                .expect("Span not found in context, this is a bug");
            let mut ext = span.extensions_mut();

            // Clear the progress bar only when the span has closed completely.
            if let Some(indicatif_ctx) = ext.get_mut::<IndicatifSpanContext>() {
                pb_manager_lock.finish_progress_bar(indicatif_ctx, &ctx);
            }
        }
    }

    // See comments on [WithContext] for why we have this.
    //
    // SAFETY: this is safe because the `WithContext` function pointer is valid
    // for the lifetime of `&self`.
    unsafe fn downcast_raw(&self, id: TypeId) -> Option<*const ()> {
        match id {
            id if id == TypeId::of::<Self>() => Some(self as *const _ as *const ()),
            id if id == TypeId::of::<WithContext>() => {
                Some(&self.get_context as *const _ as *const ())
            }
            id if id == TypeId::of::<WithStderrWriter>() => {
                Some(&self.get_stderr_writer_context as *const _ as *const ())
            }
            id if id == TypeId::of::<WithStdoutWriter>() => {
                Some(&self.get_stdout_writer_context as *const _ as *const ())
            }
            id if id == TypeId::of::<WithMultiProgress>() => {
                Some(&self.get_multi_progress_context as *const _ as *const ())
            }
            _ => None,
        }
    }
}

/// Hide all progress bars managed by [`IndicatifLayer`] (if it exists), executes `f`, then redraws
/// the progress bars. Identical to [`indicatif::MultiProgress::suspend`].
///
/// Executes `f` even if there is no default tracing subscriber or if a `IndicatifLayer` has not
/// been registered to that subscriber.
///
/// NOTE: this does not suspend stdout/stderr prints from other threads, including things like
/// `tracing::info!`. This only suspends the drawing of progress bars.
///
/// WARNING: this holds an internal lock within `MultiProgress`. Calling methods like
/// `writeln!(get_indicatif_stderr_writer(), "foobar")` or calling this method inside of `f` will
/// result in a deadlock.
pub fn suspend_tracing_indicatif<F: FnOnce() -> R, R>(f: F) -> R {
    let mut mp: Option<MultiProgress> = None;

    tracing::dispatcher::get_default(|dispatch| {
        if let Some(ctx) = dispatch.downcast_ref::<WithMultiProgress>() {
            ctx.with_context(dispatch, |fetched_mp| {
                mp = Some(fetched_mp);
            })
        }
    });

    if let Some(mp) = mp {
        mp.suspend(f)
    } else {
        f()
    }
}

/// Helper macro that allows you to print to stdout without interfering with the progress bars
/// created by tracing-indicatif.
///
/// Args are directly forwarded to `writeln!`. Do not call this macro inside of
/// `suspend_tracing_indicatif` or you will trigger a deadlock.
#[macro_export]
macro_rules! indicatif_println {
    ($($arg:tt)*) => {
        {
            use std::io::Write;

            if let Some(mut writer) = $crate::writer::get_indicatif_stdout_writer() {
                writeln!(writer, $($arg)*).unwrap();
            } else {
                #[allow(clippy::explicit_write)]
                writeln!(std::io::stdout(), $($arg)*).unwrap();
            }
        }
    }
}

/// Helper macro that allows you to print to stderr without interfering with the progress bars
/// created by tracing-indicatif.
///
/// Args are directly forwarded to `writeln!`. Do not call this macro inside of
/// `suspend_tracing_indicatif` or you will trigger a deadlock.
#[macro_export]
macro_rules! indicatif_eprintln {
    ($($arg:tt)*) => {
        {
            use std::io::Write;

            if let Some(mut writer) = $crate::writer::get_indicatif_stderr_writer() {
                writeln!(writer, $($arg)*).unwrap();
            } else {
                #[allow(clippy::explicit_write)]
                writeln!(std::io::stderr(), $($arg)*).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests;
