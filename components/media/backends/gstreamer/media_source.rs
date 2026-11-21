/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use glib::prelude::*;
use glib::translate::ToGlibPtr;
use gstreamer::ClockTime;
use gstreamer_mse::{MediaSource as NativeMediaSource, MediaSourceEOSError, MediaSourceError};
use servo_media_mse::{
    EosError, MAX_SOURCE_BUFFERS, MediaSource, MediaSourceEvent, MseError, MseResult, ReadyState,
    SourceBuffer, SourceBufferList, time_to_nanoseconds,
};
use std::any::Any;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::source_buffer::{GStreamerSourceBuffer, SourceBufferRegistry};
use crate::source_buffer_list::GStreamerSourceBufferList;

type EventCallback = Arc<dyn Fn(MediaSourceEvent) + Send + Sync>;

#[derive(Clone)]
pub struct GStreamerMediaSource {
    inner: NativeMediaSource,
    buffers: SourceBufferRegistry,
    callback: Arc<Mutex<Option<EventCallback>>>,
    creation_lock: Arc<Mutex<()>>,
}

pub(crate) fn native_error(error: glib::Error) -> MseError {
    match error.kind::<MediaSourceError>() {
        Some(MediaSourceError::InvalidState) => MseError::InvalidState,
        Some(MediaSourceError::Type) => MseError::Type,
        Some(MediaSourceError::NotSupported) => MseError::NotSupported,
        Some(MediaSourceError::NotFound) => MseError::NotFound,
        Some(MediaSourceError::QuotaExceeded) => MseError::QuotaExceeded,
        _ => MseError::Backend(error.to_string()),
    }
}

pub(crate) fn clock_time(seconds: f64) -> MseResult<ClockTime> {
    time_to_nanoseconds(seconds).map(ClockTime::from_nseconds)
}

pub(crate) fn validate_content_type(content_type: &str) -> MseResult<()> {
    if content_type.is_empty() || content_type.contains('\0') {
        return Err(MseError::Type);
    }
    if !NativeMediaSource::is_type_supported(content_type) {
        return Err(MseError::NotSupported);
    }
    Ok(())
}

impl GStreamerMediaSource {
    pub fn new() -> MseResult<Self> {
        gstreamer::init().map_err(|error| MseError::Backend(error.to_string()))?;
        if gstreamer::ElementFactory::find("msesrc").is_none() {
            return Err(MseError::NotSupported);
        }
        let inner = NativeMediaSource::new();
        let callback = Arc::new(Mutex::new(None::<EventCallback>));
        for event in [
            MediaSourceEvent::SourceOpen,
            MediaSourceEvent::SourceEnded,
            MediaSourceEvent::SourceClose,
            MediaSourceEvent::DurationChanged,
        ] {
            let callback = callback.clone();
            let notify = move |_: &NativeMediaSource| {
                let listener = callback.lock().ok().and_then(|value| value.clone());
                if let Some(listener) = listener {
                    listener(event);
                }
            };
            match event {
                MediaSourceEvent::SourceOpen => {
                    inner.connect_on_source_open(notify);
                },
                MediaSourceEvent::SourceEnded => {
                    inner.connect_on_source_ended(notify);
                },
                MediaSourceEvent::SourceClose => {
                    inner.connect_on_source_close(notify);
                },
                MediaSourceEvent::DurationChanged => {
                    inner.connect_duration_notify(notify);
                },
            }
        }
        Ok(Self {
            inner,
            buffers: Default::default(),
            callback,
            creation_lock: Default::default(),
        })
    }

    pub(crate) fn attach(&self, source: &gstreamer_mse::MseSrc) -> MseResult<()> {
        if self.ready_state()? != ReadyState::Closed {
            return Err(MseError::InvalidState);
        }
        self.inner.attach(source);
        if self.ready_state()? != ReadyState::Open {
            return Err(MseError::Backend("MediaSource did not attach".into()));
        }
        Ok(())
    }

    fn ensure_open_and_idle(&self) -> MseResult<()> {
        if self.ready_state()? != ReadyState::Open {
            return Err(MseError::InvalidState);
        }
        let buffers = self.source_buffers();
        for index in 0..buffers.length() {
            if buffers
                .index(index)?
                .is_some_and(|buffer| buffer.updating())
            {
                return Err(MseError::InvalidState);
            }
        }
        Ok(())
    }
}

impl MediaSource for GStreamerMediaSource {
    fn id(&self) -> usize {
        self.inner.as_ptr() as usize
    }
    fn clone_box(&self) -> Box<dyn MediaSource> {
        Box::new(self.clone())
    }

    fn add_source_buffer(&self, content_type: &str) -> MseResult<Box<dyn SourceBuffer>> {
        let _creation = self
            .creation_lock
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer creation lock poisoned".into()))?;
        validate_content_type(content_type)?;
        if self.ready_state()? != ReadyState::Open {
            return Err(MseError::InvalidState);
        }
        if self.inner.source_buffers().length() >= MAX_SOURCE_BUFFERS {
            return Err(MseError::QuotaExceeded);
        }
        {
            // Removed buffers may still be retained by script. Count all live
            // native handles, so repeatedly replacing buffers cannot escape the budget.
            let mut retained = self
                .buffers
                .lock()
                .map_err(|_| MseError::Backend("SourceBuffer registry lock poisoned".into()))?;
            retained.retain(|_, value| value.strong_count() > 0);
            if retained.len() >= MAX_SOURCE_BUFFERS as usize {
                return Err(MseError::QuotaExceeded);
            }
        }
        let native = self
            .inner
            .add_source_buffer(content_type)
            .map_err(native_error)?;
        Ok(Box::new(GStreamerSourceBuffer::wrap(
            native,
            &self.buffers,
        )?))
    }

    fn remove_source_buffer(&self, buffer: &dyn SourceBuffer) -> MseResult<()> {
        let native = buffer
            .as_any()
            .downcast_ref::<GStreamerSourceBuffer>()
            .ok_or(MseError::NotFound)?;
        self.inner
            .remove_source_buffer(native.inner())
            .map_err(native_error)?;
        native.mark_removed()
    }

    fn live_seekable_range(&self) -> Option<Range<f64>> {
        let mut range = gstreamer_mse::ffi::GstMediaSourceRange { start: 0, end: 0 };
        // SAFETY: inner is a live GObject and range is a valid initialized out
        // structure. Inspect raw times before constructing ClockTime, since NONE
        // is not a valid finite ClockTime and its constructor would panic.
        unsafe {
            gstreamer_mse::ffi::gst_media_source_get_live_seekable_range(
                self.inner.to_glib_none().0,
                &mut range,
            );
        }
        (range.start < range.end && range.end < u64::MAX)
            .then(|| range.start as f64 / 1e9..range.end as f64 / 1e9)
    }

    fn set_live_seekable_range(&self, start: f64, end: f64) -> MseResult<()> {
        self.ensure_open_and_idle()?;
        let start = clock_time(start)?;
        let end = clock_time(end)?;
        if start > end {
            return Err(MseError::Type);
        }
        self.inner
            .set_live_seekable_range(start, end)
            .map_err(native_error)
    }

    fn clear_live_seekable_range(&self) -> MseResult<()> {
        self.ensure_open_and_idle()?;
        self.inner.clear_live_seekable_range().map_err(native_error)
    }

    fn active_source_buffers(&self) -> Box<dyn SourceBufferList> {
        Box::new(GStreamerSourceBufferList::new(
            self.inner.active_source_buffers(),
            self.buffers.clone(),
        ))
    }
    fn source_buffers(&self) -> Box<dyn SourceBufferList> {
        Box::new(GStreamerSourceBufferList::new(
            self.inner.source_buffers(),
            self.buffers.clone(),
        ))
    }
    fn duration(&self) -> Option<f64> {
        self.inner.duration().map(|time| time.seconds_f64())
    }

    fn set_duration(&self, duration: f64) -> MseResult<()> {
        self.ensure_open_and_idle()?;
        if duration == f64::INFINITY {
            return self
                .inner
                .set_duration(ClockTime::NONE)
                .map_err(native_error);
        }
        let value = clock_time(duration)?;
        // Shrinking through buffered data requires coded-frame removal, which the
        // current native library does not implement. Never silently discard that step.
        let buffers = self.source_buffers();
        for index in 0..buffers.length() {
            if let Some(buffer) = buffers.index(index)? {
                if buffer.buffered()?.iter().any(|range| range.end > duration) {
                    return Err(MseError::NotSupported);
                }
            }
        }
        self.inner.set_duration(value).map_err(native_error)
    }

    fn end_of_stream(&self, error: Option<EosError>) -> MseResult<()> {
        self.ensure_open_and_idle()?;
        let error = match error {
            Some(EosError::Network) => MediaSourceEOSError::Network,
            Some(EosError::Decode) => MediaSourceEOSError::Decode,
            None => MediaSourceEOSError::None,
        };
        self.inner.end_of_stream(error).map_err(native_error)
    }

    fn ready_state(&self) -> MseResult<ReadyState> {
        match self.inner.ready_state() {
            gstreamer_mse::MediaSourceReadyState::Closed => Ok(ReadyState::Closed),
            gstreamer_mse::MediaSourceReadyState::Open => Ok(ReadyState::Open),
            gstreamer_mse::MediaSourceReadyState::Ended => Ok(ReadyState::Ended),
            state => Err(MseError::Backend(format!(
                "Unknown MediaSource state: {state:?}"
            ))),
        }
    }

    fn detach(&self) -> MseResult<()> {
        let buffers = self.source_buffers();
        for index in 0..buffers.length() {
            if let Some(buffer) = buffers.index(index)? {
                let buffer = buffer
                    .as_any()
                    .downcast_ref::<GStreamerSourceBuffer>()
                    .ok_or(MseError::NotFound)?;
                buffer.mark_removed()?;
            }
        }
        self.inner.detach();
        Ok(())
    }

    fn on_event(&self, callback: Box<dyn Fn(MediaSourceEvent) + Send + Sync>) {
        if let Ok(mut listener) = self.callback.lock() {
            *listener = Some(Arc::from(callback));
        }
    }
    fn clear_event_callback(&self) -> MseResult<()> {
        *self
            .callback
            .lock()
            .map_err(|_| MseError::Backend("MediaSource callback lock poisoned".into()))? = None;
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
