# Change Log

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
