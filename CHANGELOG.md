# Change Log
## 0.3.9 - 2025-01-18
* fix panic when entering a grandparent span after a child span was entered (#14)
* fix panic when using `with_span_field_formatter` (#15)

## 0.3.8 - 2024-12-01
* improve docs

## 0.3.7 - 2024-12-01
* bump dependencies
* allow for customizing tick/redraw intervals
* fix bug around footer not appearing after disappearing once
* re-export indicatif `ProgressStyle` for ease of use
* provide helper macros for printing to stdout/stderr without interfering with progress bars
* disable use of `set_move_cursor` due to regression in indicatif 0.17.9 (https://github.com/console-rs/indicatif/issues/669), this may introduce new flickering unfortunately

## 0.3.6 - 2023-12-11
* update dev dependencies (#8)

## 0.3.5 - 2023-08-21
* add method to suspend progress bars managed by IndicatifLayer, e.g. to show dialogue confirmations (closes #4)

## 0.3.4 - 2023-04-28
* add methods to fetch the `IndicatifWriter` globally if there is a default tracing subscriber and if the `IndicatifLayer` has been added

## 0.3.3 - 2023-04-27
* fix a very suble race condition that could trigger a panic (#3, thanks again @Kyuuhachi!)

## 0.3.2 - 2023-04-26
* fixed a race condition that could trigger a deadlock on span close (#2, thanks @Kyuuhachi!)

## 0.3.1 - 2023-04-25
* `inc` is now allowed to be called before `pb_start`
* added a rudimentary filter layer that allows specifying whether to show a pb or not on a per-span level

## 0.3.0 - 2023-02-18
* `get_stderr_writer` replaced `get_fmt_writer`
* added `get_stdout_writer` so one can print to stdout without interfering with progress bars
* added `IndicatifSpanExt` to be able to set per-span progress styles, support progress bars, etc
