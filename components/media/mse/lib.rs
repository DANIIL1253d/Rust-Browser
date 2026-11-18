/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Native Media Source handles. Callbacks run on media threads: embedders must
//! enqueue DOM tasks instead of accessing script objects from these callbacks.
use std::any::Any;
use std::fmt;
use std::ops::Range;

pub const MAX_APPEND_BYTES: usize = 32 * 1024 * 1024;
/// Conservative lifetime input budget until native coded-frame removal can
/// release data and report accurate retained-byte accounting.
pub const MAX_SOURCE_BUFFER_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SOURCE_BUFFERS: u32 = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MseError {
    InvalidState,
    Type,
    NotSupported,
    NotFound,
    QuotaExceeded,
    Parse,
    Backend(String),
}
impl fmt::Display for MseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for MseError {}
pub type MseResult<T> = Result<T, MseError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppendMode {
    Segments,
    Sequence,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EosError {
    Network,
    Decode,
}
pub type EndOfStreamError = EosError;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadyState {
    Closed,
    Open,
    Ended,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSourceEvent {
    SourceOpen,
    SourceEnded,
    SourceClose,
    DurationChanged,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceBufferEventKind {
    UpdateStart,
    Update,
    UpdateEnd,
    Error,
    Abort,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceBufferEvent {
    /// Identifies the append/remove which emitted this event. It remains stable
    /// through UpdateEnd, so queued events from a previous operation can be ignored.
    pub operation_id: u64,
    pub kind: SourceBufferEventKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceBufferListEvent {
    Add,
    Remove,
}

pub trait SourceBuffer: Any + Send + Sync {
    fn id(&self) -> usize;
    fn clone_box(&self) -> Box<dyn SourceBuffer>;
    fn append_mode(&self) -> MseResult<AppendMode>;
    fn set_append_mode(&self, mode: AppendMode) -> MseResult<()>;
    fn updating(&self) -> bool;
    fn abort(&self) -> MseResult<()>;
    fn timestamp_offset(&self) -> Option<f64>;
    fn set_timestamp_offset(&self, offset: f64) -> MseResult<()>;
    fn append_window_start(&self) -> Option<f64>;
    fn set_append_window_start(&self, start: f64) -> MseResult<()>;
    /// None represents an unbounded append window.
    fn append_window_end(&self) -> Option<f64>;
    fn set_append_window_end(&self, end: f64) -> MseResult<()>;
    fn buffered(&self) -> MseResult<Vec<Range<f64>>>;
    fn append_buffer(&self, data: Vec<u8>) -> MseResult<u64>;
    fn remove(&self, start: f64, end: f64) -> MseResult<u64>;
    fn change_type(&self, content_type: &str) -> MseResult<()>;
    fn on_event(&self, callback: Box<dyn Fn(SourceBufferEvent) + Send + Sync>) -> MseResult<()>;
    fn clear_event_callback(&self) -> MseResult<()>;
    fn as_any(&self) -> &dyn Any;
}
impl Clone for Box<dyn SourceBuffer> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub trait SourceBufferList: Send + Sync {
    fn length(&self) -> u32;
    fn index(&self, index: u32) -> MseResult<Option<Box<dyn SourceBuffer>>>;
    fn on_event(&self, callback: Box<dyn Fn(SourceBufferListEvent) + Send + Sync>);
    fn clear_event_callback(&self) -> MseResult<()>;
}

pub trait MediaSource: Any + Send + Sync {
    fn id(&self) -> usize;
    fn clone_box(&self) -> Box<dyn MediaSource>;
    fn add_source_buffer(&self, content_type: &str) -> MseResult<Box<dyn SourceBuffer>>;
    fn remove_source_buffer(&self, source_buffer: &dyn SourceBuffer) -> MseResult<()>;
    fn live_seekable_range(&self) -> Option<Range<f64>>;
    fn set_live_seekable_range(&self, start: f64, end: f64) -> MseResult<()>;
    fn clear_live_seekable_range(&self) -> MseResult<()>;
    fn active_source_buffers(&self) -> Box<dyn SourceBufferList>;
    fn source_buffers(&self) -> Box<dyn SourceBufferList>;
    /// None represents unknown/unbounded duration; ready_state distinguishes a
    /// closed source from an attached live source.
    fn duration(&self) -> Option<f64>;
    fn set_duration(&self, duration: f64) -> MseResult<()>;
    fn end_of_stream(&self, error: Option<EosError>) -> MseResult<()>;
    fn ready_state(&self) -> MseResult<ReadyState>;
    fn detach(&self) -> MseResult<()>;
    fn on_event(&self, callback: Box<dyn Fn(MediaSourceEvent) + Send + Sync>);
    fn clear_event_callback(&self) -> MseResult<()>;
    fn as_any(&self) -> &dyn Any;
}
impl Clone for Box<dyn MediaSource> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Validate before constructing an unsigned native clock value: NaN, infinity,
/// negative values and values overlapping the native NONE sentinel are errors.
pub fn time_to_nanoseconds(seconds: f64) -> MseResult<u64> {
    let nanoseconds = seconds * 1_000_000_000.0;
    if !seconds.is_finite() || seconds < 0.0 || nanoseconds >= u64::MAX as f64 {
        return Err(MseError::Type);
    }
    Ok(nanoseconds.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_time_conversion_rejects_invalid_and_overflowing_values() {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -0.1,
            f64::MAX,
            u64::MAX as f64,
        ] {
            assert_eq!(time_to_nanoseconds(value), Err(MseError::Type));
        }
        assert_eq!(time_to_nanoseconds(0.0), Ok(0));
        assert_eq!(time_to_nanoseconds(1.25), Ok(1_250_000_000));
        assert_eq!(time_to_nanoseconds(0.000_000_001), Ok(1));
    }
}
