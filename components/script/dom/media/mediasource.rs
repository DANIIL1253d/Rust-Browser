/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Experimental native-backed MSE. DOM list/binding scaffolding follows Servo
//! PR #47269 by decomoraes (8d3e1e53bcc1cd7d67d6aa3cad7355eed98e253a);
//! frame parsing, storage and mutation belong to servo-media-mse.
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Mutex;

use dom_struct::dom_struct;
use js::context::JSContext;
use js::rust::HandleObject;
use script_bindings::reflector::reflect_weak_referenceable_dom_object_with_proto;
use servo_media::ServoMedia;
use servo_media_mse::{
    EosError, MAX_SOURCE_BUFFERS, MediaSource as NativeMediaSource, MediaSourceEvent, MseError,
    ReadyState as NativeReadyState,
};
use stylo_atoms::Atom;

use crate::dom::bindings::codegen::Bindings::MediaSourceBinding::{
    EndOfStreamError, MediaSourceMethods, ReadyState,
};
use crate::dom::bindings::error::{Error, ErrorResult, Fallible};
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::num::Finite;
use crate::dom::bindings::refcounted::Trusted;
use crate::dom::bindings::reflector::DomGlobal;
use crate::dom::bindings::root::{Dom, DomRoot, MutNullableDom};
use crate::dom::bindings::str::DOMString;
use crate::dom::eventtarget::EventTarget;
use crate::dom::html::htmlmediaelement::HTMLMediaElement;
use crate::dom::media::sourcebuffer::SourceBuffer;
use crate::dom::media::sourcebufferlist::SourceBufferList;
use crate::dom::timeranges::TimeRangesContainer;
use crate::dom::window::Window;

pub(crate) fn mse_error(error: MseError) -> Error {
    match error {
        MseError::InvalidState => Error::InvalidState(None),
        MseError::Type => Error::Type(c"Invalid media source argument".to_owned()),
        MseError::NotSupported => Error::NotSupported(None),
        MseError::NotFound => Error::NotFound(None),
        MseError::QuotaExceeded => Error::QuotaExceeded {
            quota: None,
            requested: None,
        },
        MseError::Parse => Error::Encoding(None),
        MseError::Backend(_) => Error::Operation(None),
    }
}

pub(crate) fn time_ranges(ranges: Vec<Range<f64>>) -> Fallible<TimeRangesContainer> {
    let mut result = TimeRangesContainer::default();
    for range in ranges {
        if !range.start.is_finite()
            || !range.end.is_finite()
            || range.start < 0.
            || range.start > range.end
        {
            return Err(Error::Operation(Some(
                "Invalid native buffered range".into(),
            )));
        }
        result
            .add(range.start, range.end)
            .map_err(|_| Error::Operation(None))?;
    }
    Ok(result)
}

#[dom_struct]
pub(crate) struct MediaSource {
    eventtarget: EventTarget,
    source_buffers: Dom<SourceBufferList>,
    active_source_buffers: Dom<SourceBufferList>,
    media_element: MutNullableDom<HTMLMediaElement>,
    ready_state: Cell<ReadyState>,
    duration: Cell<f64>,
    generation: Cell<u64>,
    #[no_trace]
    #[ignore_malloc_size_of = "Native media frame store has its own bounded quota"]
    backend: Box<dyn NativeMediaSource>,
}

impl MediaSource {
    pub(crate) fn ready_state(&self) -> ReadyState {
        self.ready_state.get()
    }
    pub(crate) fn duration(&self) -> f64 {
        if self.ready_state() == ReadyState::Closed {
            return f64::NAN;
        }
        self.backend.duration().unwrap_or(self.duration.get())
    }
    pub(crate) fn native(&self) -> &dyn NativeMediaSource {
        &*self.backend
    }
    pub(crate) fn media_element_in_error(&self) -> bool {
        self.media_element.get().is_some_and(|e| e.in_error_state())
    }
    pub(crate) fn has_updating_buffer(&self) -> bool {
        self.source_buffers.buffers().iter().any(|b| b.updating())
    }
    pub(crate) fn require_open(&self) -> ErrorResult {
        if self.ready_state() == ReadyState::Open {
            Ok(())
        } else {
            Err(Error::InvalidState(None))
        }
    }

    pub(crate) fn attach(&self, element: &HTMLMediaElement) -> ErrorResult {
        if self.media_element.get().is_some() || self.ready_state() != ReadyState::Closed {
            return Err(Error::InvalidState(None));
        }
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        self.media_element.set(Some(element));
        let task_source = self
            .global()
            .task_manager()
            .media_element_task_source()
            .to_sendable();
        // Both handles are Send rather than Sync. Native listeners can run on
        // different threads; clone the handles under the lock, queue outside it.
        let callback_state = Mutex::new((Trusted::new(self), task_source));
        self.backend.on_event(Box::new(move |event| {
            if event != MediaSourceEvent::SourceOpen {
                return;
            }
            let (trusted, task_source) = {
                let Ok(state) = callback_state.lock() else {
                    return;
                };
                (state.0.clone(), state.1.clone())
            };
            task_source.queue(task!(native_media_source_open: move |cx| {
                trusted.root().opened(cx, generation);
            }));
        }));
        if element.connect_media_source(self.native()).is_err() {
            let _ = self.backend.clear_event_callback();
            self.media_element.set(None);
            let _ = self.backend.detach();
            return Err(Error::NotSupported(None));
        }
        // Some backends open synchronously during player attachment.
        if self.backend.ready_state() == Ok(NativeReadyState::Open) {
            let trusted = Trusted::new(self);
            self.global()
                .task_manager()
                .media_element_task_source()
                .queue(task!(media_source_already_open: move |cx| {
                    trusted.root().opened(cx, generation);
                }));
        }
        Ok(())
    }

    fn opened(&self, cx: &mut JSContext, generation: u64) {
        if self.generation.get() != generation
            || self.media_element.get().is_none()
            || self.ready_state() != ReadyState::Closed
        {
            return;
        }
        if self.backend.ready_state() != Ok(NativeReadyState::Open) {
            return;
        }
        let _ = self.backend.clear_event_callback(); // Release the pending-attachment Trusted root.
        self.ready_state.set(ReadyState::Open);
        if let Some(element) = self.media_element.get() {
            element.delay_load_event(cx, false);
        }
        self.upcast::<EventTarget>()
            .fire_event(cx, Atom::from("sourceopen"));
    }

    pub(crate) fn detach(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        let _ = self.backend.clear_event_callback();
        let _ = self.backend.detach();
        let was_attached = self.media_element.get().is_some();
        self.media_element.set(None);
        self.ready_state.set(ReadyState::Closed);
        self.duration.set(f64::NAN);
        for buffer in self.source_buffers.buffers() {
            buffer.detach();
        }
        if !self.active_source_buffers.is_empty() {
            self.active_source_buffers.clear();
            self.active_source_buffers.queue_removesourcebuffer_event();
        }
        if !self.source_buffers.is_empty() {
            self.source_buffers.clear();
            self.source_buffers.queue_removesourcebuffer_event();
        }
        if was_attached {
            self.queue_event("sourceclose");
        }
    }

    pub(crate) fn reopen(&self) {
        // Native append_buffer performs the corresponding native transition.
        if self.ready_state() == ReadyState::Ended
            && self.backend.ready_state() == Ok(NativeReadyState::Open)
        {
            self.ready_state.set(ReadyState::Open);
            self.queue_event("sourceopen");
        }
    }

    pub(crate) fn refresh_active_buffers(&self) {
        let native = self.backend.active_source_buffers();
        let mut ids = Vec::new();
        for index in 0..native.length() {
            if let Ok(Some(buffer)) = native.index(index) {
                ids.push(buffer.id());
            }
        }
        let active: Vec<_> = self
            .source_buffers
            .buffers()
            .into_iter()
            .filter(|b| ids.contains(&b.native_id()))
            .collect();
        self.active_source_buffers.update(&active);
    }

    pub(crate) fn buffered(&self) -> TimeRangesContainer {
        self.refresh_active_buffers();
        let mut active_ranges = Vec::new();
        let mut highest_end: f64 = 0.;
        for buffer in self.active_source_buffers.buffers() {
            let Ok(ranges) = buffer.native_ranges() else {
                return TimeRangesContainer::default();
            };
            if time_ranges(ranges.clone()).is_err() {
                return TimeRangesContainer::default();
            }
            for range in &ranges {
                highest_end = highest_end.max(range.end);
            }
            active_ranges.push(ranges);
        }
        // MSE's HTMLMediaElement buffered algorithm extends only the last range
        // for intersection at EOS, allowing slightly different A/V end times.
        // This does not add or remove native coded frames.
        if self.ready_state() == ReadyState::Ended {
            for ranges in &mut active_ranges {
                if let Some(last) = ranges.last_mut() {
                    last.end = highest_end;
                }
            }
        }
        let mut intersection: Option<Vec<Range<f64>>> = None;
        for ranges in active_ranges {
            intersection = Some(match intersection {
                None => ranges,
                Some(previous) => {
                    let mut next = Vec::new();
                    for a in previous {
                        for b in &ranges {
                            let (start, end) = (a.start.max(b.start), a.end.min(b.end));
                            if start < end {
                                next.push(start..end);
                            }
                        }
                    }
                    next
                },
            });
        }
        time_ranges(intersection.unwrap_or_default()).unwrap_or_default()
    }

    pub(crate) fn seekable(&self) -> TimeRangesContainer {
        let mut ranges = TimeRangesContainer::default();
        let duration = self.duration();
        if duration.is_finite() && duration >= 0. {
            let _ = ranges.add(0., duration);
        } else if duration == f64::INFINITY {
            let buffered = self.buffered();
            let first = buffered.start(0).ok();
            let last = buffered
                .len()
                .checked_sub(1)
                .and_then(|index| buffered.end(index).ok());
            match (self.backend.live_seekable_range(), last) {
                (Some(live), end) => {
                    let start = first.map_or(live.start, |start| live.start.min(start));
                    let end = end.map_or(live.end, |end| live.end.max(end));
                    let _ = ranges.add(start, end);
                },
                (None, Some(end)) => {
                    let _ = ranges.add(0., end);
                },
                (None, None) => {},
            }
        }
        ranges
    }

    pub(crate) fn append_failed(&self, cx: &mut JSContext) {
        let _ = self.backend.end_of_stream(Some(EosError::Decode));
        if self.backend.ready_state() == Ok(NativeReadyState::Ended) {
            self.ready_state.set(ReadyState::Ended);
            self.queue_event("sourceended");
        }
        if let Some(element) = self.media_element.get() {
            element.media_source_error(cx, EndOfStreamError::Decode);
        }
    }

    fn queue_event(&self, name: &'static str) {
        let trusted = Trusted::new(self);
        // These are events for completed DOM state transitions. Reattaching
        // before dispatch must still deliver the queued sourceclose/sourceended
        // in order. Only native callbacks are invalidated by attachment generation;
        // document destruction cancels the task source itself.
        self.global()
            .task_manager()
            .media_element_task_source()
            .queue(task!(media_source_event: move |cx| {
                let this = trusted.root();
                this.upcast::<EventTarget>().fire_event(cx, Atom::from(name));
            }));
    }
}

impl MediaSourceMethods<crate::DomTypeHolder> for MediaSource {
    fn Constructor(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
    ) -> Fallible<DomRoot<Self>> {
        let backend = ServoMedia::get().create_mse_source().map_err(mse_error)?;
        let source_buffers = SourceBufferList::new(cx, window);
        let active_source_buffers = SourceBufferList::new(cx, window);
        let source = reflect_weak_referenceable_dom_object_with_proto(
            cx,
            Rc::new(Self {
                eventtarget: EventTarget::new_inherited(),
                source_buffers: Dom::from_ref(&*source_buffers),
                active_source_buffers: Dom::from_ref(&*active_source_buffers),
                media_element: Default::default(),
                ready_state: Cell::new(ReadyState::Closed),
                duration: Cell::new(f64::NAN),
                generation: Cell::new(0),
                backend,
            }),
            window,
            proto,
        );
        window.as_global_scope().track_media_source(&source);
        Ok(source)
    }
    fn SourceBuffers(&self) -> DomRoot<SourceBufferList> {
        DomRoot::from_ref(&*self.source_buffers)
    }
    fn ActiveSourceBuffers(&self) -> DomRoot<SourceBufferList> {
        self.refresh_active_buffers();
        DomRoot::from_ref(&*self.active_source_buffers)
    }
    fn ReadyState(&self) -> ReadyState {
        self.ready_state()
    }
    fn Duration(&self) -> f64 {
        self.duration()
    }
    fn SetDuration(&self, value: f64) -> ErrorResult {
        if value.is_nan() || value < 0. {
            return Err(Error::Type(c"Invalid duration".to_owned()));
        }
        self.require_open()?;
        if self.has_updating_buffer() {
            return Err(Error::InvalidState(None));
        }
        self.backend.set_duration(value).map_err(mse_error)?;
        self.duration.set(value);
        if let Some(element) = self.media_element.get() {
            element.media_source_duration_changed(value);
        }
        Ok(())
    }
    fn CanConstructInDedicatedWorker(_window: &Window) -> bool {
        false
    }
    fn AddSourceBuffer(
        &self,
        cx: &mut JSContext,
        type_: DOMString,
    ) -> Fallible<DomRoot<SourceBuffer>> {
        if type_.is_empty() {
            return Err(Error::Type(c"Empty content type".to_owned()));
        }
        if !ServoMedia::get().is_mse_type_supported(&type_.str()) {
            return Err(Error::NotSupported(None));
        }
        self.require_open()?;
        if self.source_buffers.len() >= MAX_SOURCE_BUFFERS as usize {
            return Err(mse_error(MseError::QuotaExceeded));
        }
        let native = self
            .backend
            .add_source_buffer(&type_.str())
            .map_err(mse_error)?;
        // Retain a native handle until DOM reflection succeeds, so an invalid
        // backend enum cannot leave an invisible buffer in the native list.
        let buffer = match SourceBuffer::new(cx, self, native.clone()) {
            Ok(buffer) => buffer,
            Err(error) => {
                let _ = self.backend.remove_source_buffer(&*native);
                return Err(error);
            },
        };
        self.source_buffers.add(&buffer);
        Ok(buffer)
    }
    fn RemoveSourceBuffer(&self, buffer: &SourceBuffer) -> ErrorResult {
        if !self.source_buffers.contains(buffer) {
            return Err(Error::NotFound(None));
        }
        self.backend
            .remove_source_buffer(buffer.native())
            .map_err(mse_error)?;
        buffer.detach();
        self.active_source_buffers.remove(buffer);
        self.source_buffers.remove(buffer);
        Ok(())
    }
    fn EndOfStream(&self, cx: &mut JSContext, error: Option<EndOfStreamError>) -> ErrorResult {
        self.require_open()?;
        if self.has_updating_buffer() {
            return Err(Error::InvalidState(None));
        }
        let native_error = error.map(|e| match e {
            EndOfStreamError::Network => EosError::Network,
            EndOfStreamError::Decode => EosError::Decode,
        });
        self.backend
            .end_of_stream(native_error)
            .map_err(mse_error)?;
        self.ready_state.set(ReadyState::Ended);
        self.queue_event("sourceended");
        if let (Some(element), Some(error)) = (self.media_element.get(), error) {
            element.media_source_error(cx, error);
        }
        Ok(())
    }
    fn SetLiveSeekableRange(&self, start: Finite<f64>, end: Finite<f64>) -> ErrorResult {
        self.require_open()?;
        if *start < 0. || *end < *start {
            return Err(Error::Type(c"Invalid live seekable range".to_owned()));
        }
        self.backend
            .set_live_seekable_range(*start, *end)
            .map_err(mse_error)
    }
    fn ClearLiveSeekableRange(&self) -> ErrorResult {
        self.require_open()?;
        self.backend.clear_live_seekable_range().map_err(mse_error)
    }
    fn IsTypeSupported(_window: &Window, type_: DOMString) -> bool {
        ServoMedia::get().is_mse_type_supported(&type_.str())
    }
    event_handler!(sourceopen, GetOnsourceopen, SetOnsourceopen);
    event_handler!(sourceended, GetOnsourceended, SetOnsourceended);
    event_handler!(sourceclose, GetOnsourceclose, SetOnsourceclose);
}
