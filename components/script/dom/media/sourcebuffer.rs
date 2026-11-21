/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! DOM scaffolding follows Servo PR #47269. Every coded-frame operation and
//! buffered range is supplied by the native backend, not a JavaScript-side parser.
//! Original MPL scaffolding: decomoraes, 8d3e1e53bcc1cd7d67d6aa3cad7355eed98e253a.
use std::cell::Cell;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dom_struct::dom_struct;
use js::context::JSContext;
use script_bindings::reflector::reflect_dom_object;
use servo_media::ServoMedia;
use servo_media_mse::{
    AppendMode as NativeAppendMode, MAX_APPEND_BYTES, SourceBuffer as NativeSourceBuffer,
    SourceBufferEvent, SourceBufferEventKind,
};
use stylo_atoms::Atom;

use crate::dom::bindings::buffer_source::get_buffer_source_copy;
use crate::dom::bindings::codegen::Bindings::SourceBufferBinding::{
    AppendMode, SourceBufferMethods,
};
use crate::dom::bindings::codegen::UnionTypes::ArrayBufferViewOrArrayBuffer;
use crate::dom::bindings::error::{Error, ErrorResult, Fallible};
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::num::Finite;
use crate::dom::bindings::refcounted::Trusted;
use crate::dom::bindings::reflector::DomGlobal;
use crate::dom::bindings::root::{DomRoot, MutNullableDom};
use crate::dom::bindings::str::DOMString;
use crate::dom::eventtarget::EventTarget;
use crate::dom::media::mediasource::{MediaSource, mse_error, time_ranges};
use crate::dom::timeranges::TimeRanges;

#[dom_struct]
pub(crate) struct SourceBuffer {
    eventtarget: EventTarget,
    media_source: MutNullableDom<MediaSource>,
    updating: Cell<bool>,
    generation: Cell<u64>,
    operation: Cell<Option<u64>>,
    completed_operation: Cell<Option<u64>>,
    failed: Cell<bool>,
    mode: Cell<AppendMode>,
    #[no_trace]
    #[ignore_malloc_size_of = "Native source buffer enforces its own coded-frame quota"]
    backend: Box<dyn NativeSourceBuffer>,
}

impl SourceBuffer {
    pub(crate) fn new(
        cx: &mut JSContext,
        source: &MediaSource,
        backend: Box<dyn NativeSourceBuffer>,
    ) -> Fallible<DomRoot<Self>> {
        let mode = match backend.append_mode().map_err(mse_error)? {
            NativeAppendMode::Segments => AppendMode::Segments,
            NativeAppendMode::Sequence => AppendMode::Sequence,
        };
        Ok(reflect_dom_object(
            cx,
            Box::new(Self {
                eventtarget: EventTarget::new_inherited(),
                media_source: MutNullableDom::new(Some(source)),
                updating: Cell::new(false),
                generation: Cell::new(0),
                operation: Cell::new(None),
                completed_operation: Cell::new(None),
                failed: Cell::new(false),
                mode: Cell::new(mode),
                backend,
            }),
            source.global().as_window(),
        ))
    }
    pub(crate) fn native(&self) -> &dyn NativeSourceBuffer {
        &*self.backend
    }
    pub(crate) fn native_id(&self) -> usize {
        self.backend.id()
    }
    pub(crate) fn native_ranges(&self) -> Fallible<Vec<Range<f64>>> {
        self.backend.buffered().map_err(mse_error)
    }
    pub(crate) fn updating(&self) -> bool {
        self.updating.get() || self.backend.updating()
    }

    fn parent(&self) -> Fallible<DomRoot<MediaSource>> {
        self.media_source.get().ok_or(Error::InvalidState(None))
    }
    fn prepare(&self) -> Fallible<DomRoot<MediaSource>> {
        let source = self.parent()?;
        if self.updating()
            || source.ready_state()
                == crate::dom::bindings::codegen::Bindings::MediaSourceBinding::ReadyState::Closed
        {
            return Err(Error::InvalidState(None));
        }
        if source.media_element_in_error() {
            return Err(Error::InvalidState(None));
        }
        Ok(source)
    }

    pub(crate) fn detach(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        let _ = self.backend.clear_event_callback();
        self.operation.set(None);
        self.completed_operation.set(None);
        self.media_source.set(None);
        if self.updating.replace(false) {
            self.queue_event("abort");
            self.queue_event("updateend");
        }
    }

    fn install_operation_callback(&self) -> ErrorResult {
        let generation = self.generation.get();
        let task_source = self
            .global()
            .task_manager()
            .media_element_task_source()
            .to_sendable();
        let callback_state = Mutex::new((Trusted::new(self), task_source));
        self.backend
            .on_event(Box::new(move |event| {
                let (trusted, task_source) = {
                    let Ok(state) = callback_state.lock() else {
                        return;
                    };
                    (state.0.clone(), state.1.clone())
                };
                task_source.queue(task!(native_source_buffer_event: move |cx| {
                    let this = trusted.root();
                    if this.generation.get() == generation { this.handle_event(cx, event); }
                }));
            }))
            .map_err(mse_error)
    }

    fn start_operation(
        &self,
        operation: impl FnOnce(&dyn NativeSourceBuffer) -> servo_media_mse::MseResult<u64>,
    ) -> ErrorResult {
        self.install_operation_callback()?;
        self.updating.set(true);
        self.failed.set(false);
        // Queue before invoking native code, whose callbacks may already be queued
        // when the call returns. Synchronous rejection must emit no updatestart.
        let accepted = Arc::new(AtomicBool::new(false));
        let accepted_task = accepted.clone();
        let trusted = Trusted::new(self);
        let generation = self.generation.get();
        self.global()
            .task_manager()
            .media_element_task_source()
            .queue(task!(source_buffer_update_start: move |cx| {
                let this = trusted.root();
                if accepted_task.load(Ordering::Acquire) && this.generation.get() == generation {
                    this.upcast::<EventTarget>().fire_event(cx, Atom::from("updatestart"));
                }
            }));
        match operation(self.native()) {
            Ok(id) => {
                self.operation.set(Some(id));
                accepted.store(true, Ordering::Release);
                Ok(())
            },
            Err(error) => {
                self.updating.set(false);
                let _ = self.backend.clear_event_callback();
                Err(mse_error(error))
            },
        }
    }

    fn handle_event(&self, cx: &mut JSContext, event: SourceBufferEvent) {
        let current = self.operation.get() == Some(event.operation_id);
        // An update handler may begin the next append before the previous queued
        // updateend. Preserve that event without mutating the newer operation.
        let completed = self.completed_operation.get() == Some(event.operation_id);
        if !current && !(completed && event.kind == SourceBufferEventKind::UpdateEnd) {
            return;
        }
        match event.kind {
            SourceBufferEventKind::UpdateStart => {}, // DOM queued this before the native append.
            SourceBufferEventKind::Update => {
                self.updating.set(false);
                self.operation.set(None);
                self.completed_operation.set(Some(event.operation_id));
                if !self.failed.get() {
                    self.upcast::<EventTarget>()
                        .fire_event(cx, Atom::from("update"));
                }
            },
            SourceBufferEventKind::Error => {
                self.failed.set(true);
                self.updating.set(false);
                self.operation.set(None);
                self.completed_operation.set(Some(event.operation_id));
                self.upcast::<EventTarget>()
                    .fire_event(cx, Atom::from("error"));
                if let Some(source) = self.media_source.get() {
                    source.append_failed(cx);
                }
            },
            SourceBufferEventKind::Abort => {
                self.updating.set(false);
                self.operation.set(None);
                self.completed_operation.set(Some(event.operation_id));
                self.upcast::<EventTarget>()
                    .fire_event(cx, Atom::from("abort"));
            },
            SourceBufferEventKind::UpdateEnd => {
                if current {
                    self.updating.set(false);
                    self.operation.set(None);
                }
                if completed {
                    self.completed_operation.set(None);
                }
                if self.operation.get().is_none() {
                    let _ = self.backend.clear_event_callback();
                }
                if let Some(source) = self.media_source.get() {
                    source.refresh_active_buffers();
                }
                self.upcast::<EventTarget>()
                    .fire_event(cx, Atom::from("updateend"));
            },
        }
    }

    fn queue_event(&self, name: &'static str) {
        let trusted = Trusted::new(self);
        let generation = self.generation.get();
        self.global().task_manager().media_element_task_source().queue(task!(source_buffer_event: move |cx| {
            let this = trusted.root();
            if this.generation.get() == generation { this.upcast::<EventTarget>().fire_event(cx, Atom::from(name)); }
        }));
    }
}

impl SourceBufferMethods<crate::DomTypeHolder> for SourceBuffer {
    fn Mode(&self) -> AppendMode {
        self.mode.get()
    }
    fn SetMode(&self, value: AppendMode) -> ErrorResult {
        let source = self.prepare()?;
        self.backend
            .set_append_mode(match value {
                AppendMode::Segments => NativeAppendMode::Segments,
                AppendMode::Sequence => NativeAppendMode::Sequence,
            })
            .map_err(mse_error)?;
        self.mode.set(value);
        source.reopen();
        Ok(())
    }
    fn Updating(&self) -> bool {
        self.updating()
    }
    fn GetBuffered(&self, cx: &mut JSContext) -> Fallible<DomRoot<TimeRanges>> {
        self.parent()?;
        let ranges = time_ranges(self.native_ranges()?)?;
        Ok(TimeRanges::new(cx, self.global().as_window(), ranges))
    }
    fn TimestampOffset(&self) -> Finite<f64> {
        Finite::wrap(self.backend.timestamp_offset().unwrap_or(0.))
    }
    fn SetTimestampOffset(&self, value: Finite<f64>) -> ErrorResult {
        let source = self.prepare()?;
        self.backend
            .set_timestamp_offset(*value)
            .map_err(mse_error)?;
        source.reopen();
        Ok(())
    }
    fn AppendWindowStart(&self) -> Finite<f64> {
        Finite::wrap(self.backend.append_window_start().unwrap_or(0.))
    }
    fn SetAppendWindowStart(&self, value: Finite<f64>) -> ErrorResult {
        self.prepare()?;
        if *value < 0. || *value >= self.AppendWindowEnd() {
            return Err(Error::Type(c"Invalid append window start".to_owned()));
        }
        self.backend
            .set_append_window_start(*value)
            .map_err(mse_error)
    }
    fn AppendWindowEnd(&self) -> f64 {
        self.backend.append_window_end().unwrap_or(f64::INFINITY)
    }
    fn SetAppendWindowEnd(&self, value: f64) -> ErrorResult {
        self.prepare()?;
        if value.is_nan() || value <= *self.AppendWindowStart() {
            return Err(Error::Type(c"Invalid append window end".to_owned()));
        }
        self.backend.set_append_window_end(value).map_err(mse_error)
    }
    fn AppendBuffer(&self, data: ArrayBufferViewOrArrayBuffer) -> ErrorResult {
        let source = self.prepare()?;
        let length = match &data {
            ArrayBufferViewOrArrayBuffer::ArrayBufferView(view) => view.len(),
            ArrayBufferViewOrArrayBuffer::ArrayBuffer(buffer) => buffer.len(),
        };
        if length > MAX_APPEND_BYTES {
            return Err(Error::QuotaExceeded {
                quota: None,
                requested: None,
            });
        }
        let bytes = get_buffer_source_copy((&data).into());
        self.start_operation(move |backend| backend.append_buffer(bytes))?;
        source.reopen();
        Ok(())
    }
    fn Abort(&self) -> ErrorResult {
        let source = self.parent()?;
        source.require_open()?;
        // The native 1.28 adapter returns NotSupported until real parser cancellation
        // exists. Do not synthesize a successful abort for an operation still running.
        self.backend.abort().map_err(mse_error)
    }
    fn ChangeType(&self, type_: DOMString) -> ErrorResult {
        let source = self.prepare()?;
        if type_.is_empty() {
            return Err(Error::Type(c"Empty content type".to_owned()));
        }
        if !ServoMedia::get().is_mse_type_supported(&type_.str()) {
            return Err(Error::NotSupported(None));
        }
        self.backend.change_type(&type_.str()).map_err(mse_error)?;
        self.mode
            .set(match self.backend.append_mode().map_err(mse_error)? {
                NativeAppendMode::Segments => AppendMode::Segments,
                NativeAppendMode::Sequence => AppendMode::Sequence,
            });
        source.reopen();
        Ok(())
    }
    fn Remove(&self, start: Finite<f64>, end: f64) -> ErrorResult {
        let source = self.prepare()?;
        let duration = source.duration();
        if duration.is_nan() || *start < 0. || *start > duration || end.is_nan() || end <= *start {
            return Err(Error::Type(c"Invalid removal range".to_owned()));
        }
        self.start_operation(|backend| backend.remove(*start, end))?;
        source.reopen();
        Ok(())
    }
    event_handler!(updatestart, GetOnupdatestart, SetOnupdatestart);
    event_handler!(update, GetOnupdate, SetOnupdate);
    event_handler!(updateend, GetOnupdateend, SetOnupdateend);
    event_handler!(error, GetOnerror, SetOnerror);
    event_handler!(abort, GetOnabort, SetOnabort);
}
