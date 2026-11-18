/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use glib::prelude::*;
use glib::translate::{ToGlibPtr, from_glib_full};
use gstreamer_mse::{SourceBuffer as NativeSourceBuffer, SourceBufferAppendMode};
use servo_media_mse::{
    AppendMode, MAX_APPEND_BYTES, MAX_SOURCE_BUFFER_BYTES, MseError, MseResult, SourceBuffer,
    SourceBufferEvent, SourceBufferEventKind,
};
use std::any::Any;
use std::collections::HashMap;
use std::ops::Range;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, Weak};

use crate::media_source::{clock_time, native_error, validate_content_type};

type EventCallback = Arc<dyn Fn(SourceBufferEvent) + Send + Sync>;

#[derive(Default)]
pub(crate) struct BufferEvents {
    next_id: u64,
    active: Option<u64>,
    removed: bool,
    callback: Option<EventCallback>,
    accepted_bytes: usize,
}
pub(crate) type SourceBufferRegistry = Arc<Mutex<HashMap<usize, Weak<Mutex<BufferEvents>>>>>;

#[derive(Clone)]
pub struct GStreamerSourceBuffer {
    inner: NativeSourceBuffer,
    events: Arc<Mutex<BufferEvents>>,
}

impl GStreamerSourceBuffer {
    pub(crate) fn inner(&self) -> &NativeSourceBuffer {
        &self.inner
    }

    pub(crate) fn wrap(
        inner: NativeSourceBuffer,
        registry: &SourceBufferRegistry,
    ) -> MseResult<Self> {
        let id = inner.as_ptr() as usize;
        let mut registry = registry
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer registry lock poisoned".into()))?;
        registry.retain(|_, value| value.strong_count() > 0);
        if let Some(events) = registry.get(&id).and_then(Weak::upgrade) {
            return Ok(Self { inner, events });
        }
        let events = Arc::new(Mutex::new(BufferEvents::default()));
        for kind in [
            SourceBufferEventKind::UpdateStart,
            SourceBufferEventKind::Update,
            SourceBufferEventKind::UpdateEnd,
            SourceBufferEventKind::Error,
            SourceBufferEventKind::Abort,
        ] {
            let state = events.clone();
            let notify = move |_: &NativeSourceBuffer| {
                let dispatch = {
                    let Ok(mut events) = state.lock() else {
                        return;
                    };
                    if events.removed {
                        return;
                    }
                    let Some(operation_id) = events.active else {
                        return;
                    };
                    let callback = events.callback.clone();
                    if kind == SourceBufferEventKind::UpdateEnd {
                        events.active = None;
                        events.callback = None;
                    }
                    callback.map(|callback| (callback, SourceBufferEvent { operation_id, kind }))
                };
                if let Some((callback, event)) = dispatch {
                    callback(event);
                }
            };
            match kind {
                SourceBufferEventKind::UpdateStart => {
                    inner.connect_on_update_start(notify);
                },
                SourceBufferEventKind::Update => {
                    inner.connect_on_update(notify);
                },
                SourceBufferEventKind::UpdateEnd => {
                    inner.connect_on_update_end(notify);
                },
                SourceBufferEventKind::Error => {
                    inner.connect_on_error(notify);
                },
                SourceBufferEventKind::Abort => {
                    inner.connect_on_abort(notify);
                },
            }
        }
        registry.insert(id, Arc::downgrade(&events));
        Ok(Self { inner, events })
    }

    pub(crate) fn mark_removed(&self) -> MseResult<()> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?;
        events.removed = true;
        events.active = None;
        events.callback = None;
        Ok(())
    }

    fn ensure_idle(&self) -> MseResult<()> {
        let events = self
            .events
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?;
        if events.removed || events.active.is_some() || self.inner.is_updating() {
            return Err(MseError::InvalidState);
        }
        Ok(())
    }
}

/// Own the transfer-full GArray returned by gst_source_buffer_get_buffered.
/// gstreamer-mse 0.25 does not provide its safe Rust conversion yet.
struct NativeRanges(NonNull<glib_sys::GArray>);
impl Drop for NativeRanges {
    fn drop(&mut self) {
        // SAFETY: this is the single owning reference returned by the native getter.
        unsafe {
            glib_sys::g_array_unref(self.0.as_ptr());
        }
    }
}

impl SourceBuffer for GStreamerSourceBuffer {
    fn id(&self) -> usize {
        self.inner.as_ptr() as usize
    }
    fn clone_box(&self) -> Box<dyn SourceBuffer> {
        Box::new(self.clone())
    }

    fn append_mode(&self) -> MseResult<AppendMode> {
        match self.inner.append_mode() {
            SourceBufferAppendMode::Segments => Ok(AppendMode::Segments),
            SourceBufferAppendMode::Sequence => Ok(AppendMode::Sequence),
            mode => Err(MseError::Backend(format!(
                "Unknown SourceBuffer append mode: {mode:?}"
            ))),
        }
    }
    fn set_append_mode(&self, mode: AppendMode) -> MseResult<()> {
        self.ensure_idle()?;
        if mode != self.append_mode()?
            && self
                .events
                .lock()
                .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?
                .accepted_bytes
                > 0
        {
            // Native set_append_mode only updates the SourceBuffer property; it
            // configures existing track timestamp processors on the next init
            // segment. Reject changing a live timeline until that transition is implemented.
            return Err(MseError::NotSupported);
        }
        if mode == AppendMode::Segments
            && self
                .inner
                .timestamp_offset()
                .is_some_and(|offset| offset.nseconds() != 0)
        {
            return Err(MseError::NotSupported);
        }
        self.inner
            .set_append_mode(match mode {
                AppendMode::Segments => SourceBufferAppendMode::Segments,
                AppendMode::Sequence => SourceBufferAppendMode::Sequence,
            })
            .map_err(native_error)
    }
    fn updating(&self) -> bool {
        self.events.lock().map_or(true, |events| {
            !events.removed && (events.active.is_some() || self.inner.is_updating())
        })
    }
    fn abort(&self) -> MseResult<()> {
        // 1.28.8 merely feeds EOS to the parser here; it does not reset/abort the
        // operation or emit the specified abort/updateend sequence.
        if self
            .events
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?
            .removed
        {
            return Err(MseError::InvalidState);
        }
        Err(MseError::NotSupported)
    }
    fn timestamp_offset(&self) -> Option<f64> {
        self.inner.timestamp_offset().map(|time| time.seconds_f64())
    }
    fn set_timestamp_offset(&self, offset: f64) -> MseResult<()> {
        self.ensure_idle()?;
        if offset.is_finite() && offset < 0.0 {
            return Err(MseError::NotSupported);
        }
        // Native 1.28.8 only applies timestamp offsets in sequence mode. In
        // segments mode its setter changes the property but leaves parsed PTS/DTS
        // untouched; reporting success would expose incorrect timeline semantics.
        if offset != 0.0 && offset.is_finite() && self.append_mode()? == AppendMode::Segments {
            return Err(MseError::NotSupported);
        }
        self.inner
            .set_timestamp_offset(clock_time(offset)?)
            .map_err(native_error)
    }
    fn append_window_start(&self) -> Option<f64> {
        self.inner
            .append_window_start()
            .map(|time| time.seconds_f64())
    }
    fn set_append_window_start(&self, start: f64) -> MseResult<()> {
        self.ensure_idle()?;
        let start = clock_time(start)?;
        if self
            .inner
            .append_window_end()
            .is_some_and(|end| start >= end)
        {
            return Err(MseError::Type);
        }
        // Native 1.28.8 reverses this bound check: it rejects all valid starts
        // with an unbounded end, and can accept starts after a finite end.
        // Preserve validation without exposing the broken native mutation.
        Err(MseError::NotSupported)
    }
    fn append_window_end(&self) -> Option<f64> {
        self.inner
            .append_window_end()
            .map(|time| time.seconds_f64())
    }
    fn set_append_window_end(&self, end: f64) -> MseResult<()> {
        self.ensure_idle()?;
        let time = if end == f64::INFINITY {
            None
        } else {
            Some(clock_time(end)?)
        };
        self.inner.set_append_window_end(time).map_err(native_error)
    }

    fn buffered(&self) -> MseResult<Vec<Range<f64>>> {
        if self
            .events
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?
            .removed
        {
            return Err(MseError::InvalidState);
        }
        let mut error = std::ptr::null_mut();
        // SAFETY: inner is a live GObject; error is an out pointer, and the result
        // is a transfer-full GArray of GstMediaSourceRange values (native API contract).
        let raw = unsafe {
            gstreamer_mse::ffi::gst_source_buffer_get_buffered(
                self.inner.to_glib_none().0,
                &mut error,
            )
        };
        let ranges = NonNull::new(raw).map(NativeRanges);
        if !error.is_null() {
            // SAFETY: a non-null GError out pointer transfers ownership to the caller.
            return Err(native_error(unsafe { from_glib_full(error) }));
        }
        let ranges = ranges
            .ok_or_else(|| MseError::Backend("Native buffered range array is missing".into()))?;
        // SAFETY: the owned array remains alive until all elements have been copied.
        unsafe {
            let array = ranges.0.as_ref();
            if glib_sys::g_array_get_element_size(ranges.0.as_ptr()) as usize
                != std::mem::size_of::<gstreamer_mse::ffi::GstMediaSourceRange>()
            {
                return Err(MseError::Backend(
                    "Unexpected native buffered range element size".into(),
                ));
            }
            if array.len == 0 {
                return Ok(Vec::new());
            }
            if array.data.is_null() {
                return Err(MseError::Backend(
                    "Native buffered range data is missing".into(),
                ));
            }
            let length = array.len as usize;
            if length
                > isize::MAX as usize
                    / std::mem::size_of::<gstreamer_mse::ffi::GstMediaSourceRange>()
            {
                return Err(MseError::Backend(
                    "Native buffered range array is too large".into(),
                ));
            }
            let values = std::slice::from_raw_parts(
                array.data.cast::<gstreamer_mse::ffi::GstMediaSourceRange>(),
                length,
            );
            values
                .iter()
                .map(|range| {
                    if range.start > range.end || range.end == u64::MAX {
                        return Err(MseError::Backend(
                            "Invalid native buffered time range".into(),
                        ));
                    }
                    Ok(range.start as f64 / 1e9..range.end as f64 / 1e9)
                })
                .collect()
        }
    }

    fn append_buffer(&self, data: Vec<u8>) -> MseResult<u64> {
        if data.len() > MAX_APPEND_BYTES {
            return Err(MseError::QuotaExceeded);
        }
        let size = data.len();
        let operation_id = {
            let mut events = self
                .events
                .lock()
                .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?;
            if events.removed || events.active.is_some() || self.inner.is_updating() {
                return Err(MseError::InvalidState);
            }
            let accepted = events
                .accepted_bytes
                .checked_add(size)
                .ok_or(MseError::QuotaExceeded)?;
            if accepted > MAX_SOURCE_BUFFER_BYTES {
                return Err(MseError::QuotaExceeded);
            }
            let operation_id = events.next_id.checked_add(1).ok_or_else(|| {
                MseError::Backend("SourceBuffer operation counter exhausted".into())
            })?;
            events.next_id = operation_id;
            events.active = Some(operation_id);
            events.accepted_bytes = accepted;
            operation_id
        };
        if let Err(error) = self
            .inner
            .append_buffer(gstreamer::Buffer::from_slice(data))
        {
            if let Ok(mut events) = self.events.lock() {
                if events.active == Some(operation_id) {
                    events.active = None;
                    events.callback = None;
                    events.accepted_bytes = events.accepted_bytes.saturating_sub(size);
                }
            }
            return Err(native_error(error));
        }
        Ok(operation_id)
    }
    fn remove(&self, start: f64, end: f64) -> MseResult<u64> {
        self.ensure_idle()?;
        let start = clock_time(start)?;
        if end != f64::INFINITY && start >= clock_time(end)? {
            return Err(MseError::Type);
        }
        // GStreamer 1.28.8 gst_source_buffer_remove() is a no-op which returns TRUE.
        // Forwarding it would report successful removal while retaining every sample.
        Err(MseError::NotSupported)
    }
    fn change_type(&self, content_type: &str) -> MseResult<()> {
        self.ensure_idle()?;
        validate_content_type(content_type)?;
        self.inner
            .change_content_type(content_type)
            .map_err(native_error)
    }
    fn on_event(&self, callback: Box<dyn Fn(SourceBufferEvent) + Send + Sync>) -> MseResult<()> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?;
        if events.removed {
            return Err(MseError::InvalidState);
        }
        events.callback = Some(Arc::from(callback));
        Ok(())
    }
    fn clear_event_callback(&self) -> MseResult<()> {
        self.events
            .lock()
            .map_err(|_| MseError::Backend("SourceBuffer state lock poisoned".into()))?
            .callback = None;
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
