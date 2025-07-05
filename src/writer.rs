//! Helpers to prevent progress bars from clobbering your console output.
use std::io;
use std::marker::PhantomData;

use indicatif::MultiProgress;
use tracing_subscriber::fmt::MakeWriter;

pub trait WriterTarget: private::Sealed {}

/// Marker for where the [`IndicatifWriter`] should write to.
pub struct Stdout {}
/// Marker for where the [`IndicatifWriter`] should write to.
pub struct Stderr {}

impl WriterTarget for Stdout {}
impl WriterTarget for Stderr {}

// TODO(emersonford): find a cleaner way to integrate this layer with fmt::Layer.
/// A wrapper around [`std::io::stdout()`] or [`std::io::stderr()`] to ensure that output to either is
/// not clobbered by active progress bars. This should be passed into tracing fmt's layer so
/// tracing log entries are not clobbered.
pub struct IndicatifWriter<Target = Stderr> {
    progress_bars: MultiProgress,
    inner: PhantomData<Target>,
}

impl<T> IndicatifWriter<T>
where
    T: WriterTarget,
{
    /// Implementation of [`MakeWriter`] that wraps around an existing [`MultiProgress`] and calls
    /// [`MultiProgress::suspend`] anytime tracing writes to this.
    ///
    /// Recommended you obtain this using
    /// [`IndicatifLayer::get_stderr_writer`](crate::IndicatifLayer::get_stderr_writer) instead of
    /// this method.
    pub fn new(mp: MultiProgress) -> Self {
        Self {
            progress_bars: mp,
            inner: PhantomData,
        }
    }
}

impl<T> Clone for IndicatifWriter<T> {
    fn clone(&self) -> Self {
        Self {
            progress_bars: self.progress_bars.clone(),
            inner: self.inner,
        }
    }
}

impl io::Write for IndicatifWriter<Stdout> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress_bars.suspend(|| io::stdout().write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stdout().flush())
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.progress_bars
            .suspend(|| io::stdout().write_vectored(bufs))
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stdout().write_all(buf))
    }

    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stdout().write_fmt(fmt))
    }
}

impl io::Write for IndicatifWriter<Stderr> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress_bars.suspend(|| io::stderr().write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stderr().flush())
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.progress_bars
            .suspend(|| io::stderr().write_vectored(bufs))
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stderr().write_all(buf))
    }

    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> io::Result<()> {
        self.progress_bars.suspend(|| io::stderr().write_fmt(fmt))
    }
}

impl<'a> MakeWriter<'a> for IndicatifWriter<Stdout> {
    type Writer = IndicatifWriter<Stdout>;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl<'a> MakeWriter<'a> for IndicatifWriter<Stderr> {
    type Writer = IndicatifWriter<Stderr>;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

mod private {
    pub trait Sealed {}

    impl Sealed for super::Stdout {}
    impl Sealed for super::Stderr {}
}

/// Returns the stderr writer (equivalent to
/// [`get_stderr_writer`](crate::IndicatifLayer::get_stderr_writer)) of the registered
/// [`IndicatifLayer`](crate::IndicatifLayer) for the current default tracing subscriber.
///
/// Returns `None` if there is either no default tracing subscriber or if there is not a
/// `IndicatifLayer` registered with that subscriber.
pub fn get_indicatif_stderr_writer() -> Option<IndicatifWriter<Stderr>> {
    tracing::dispatcher::get_default(|dispatch| {
        dispatch
            .downcast_ref::<crate::WithStderrWriter>()
            .and_then(|ctx| {
                let mut ret: Option<IndicatifWriter<Stderr>> = None;
                ctx.with_context(dispatch, |writer| {
                    ret = Some(writer);
                });

                ret
            })
    })
}

/// Returns the stdout writer (equivalent to
/// [`get_stdout_writer`](crate::IndicatifLayer::get_stdout_writer)) of the registered
/// [`IndicatifLayer`](crate::IndicatifLayer) for the current default tracing subscriber.
///
/// Returns `None` if there is either no default tracing subscriber or if there is not a
/// `IndicatifLayer` registered with that subscriber.
pub fn get_indicatif_stdout_writer() -> Option<IndicatifWriter<Stdout>> {
    tracing::dispatcher::get_default(|dispatch| {
        dispatch
            .downcast_ref::<crate::WithStdoutWriter>()
            .and_then(|ctx| {
                let mut ret: Option<IndicatifWriter<Stdout>> = None;
                ctx.with_context(dispatch, |writer| {
                    ret = Some(writer);
                });

                ret
            })
    })
}
