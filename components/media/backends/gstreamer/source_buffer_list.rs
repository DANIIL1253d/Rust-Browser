/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::source_buffer::{GStreamerSourceBuffer, SourceBufferRegistry};
use glib::prelude::*;
use servo_media_mse::{MseError, MseResult, SourceBuffer, SourceBufferList, SourceBufferListEvent};
use std::sync::{Arc, Mutex};

type EventCallback = Arc<dyn Fn(SourceBufferListEvent) + Send + Sync>;

pub struct GStreamerSourceBufferList {
    inner: gstreamer_mse::SourceBufferList,
    buffers: SourceBufferRegistry,
    callback: Arc<Mutex<Option<EventCallback>>>,
    handlers: Vec<glib::SignalHandlerId>,
}
impl GStreamerSourceBufferList {
    pub(crate) fn new(
        inner: gstreamer_mse::SourceBufferList,
        buffers: SourceBufferRegistry,
    ) -> Self {
        let callback = Arc::new(Mutex::new(None::<EventCallback>));
        let added = Arc::downgrade(&callback);
        let removed = added.clone();
        let handlers = vec![
            inner.connect_on_sourcebuffer_added(move |_| {
                let listener = added
                    .upgrade()
                    .and_then(|value| value.lock().ok().and_then(|value| value.clone()));
                if let Some(listener) = listener {
                    listener(SourceBufferListEvent::Add);
                }
            }),
            inner.connect_on_sourcebuffer_removed(move |_| {
                let listener = removed
                    .upgrade()
                    .and_then(|value| value.lock().ok().and_then(|value| value.clone()));
                if let Some(listener) = listener {
                    listener(SourceBufferListEvent::Remove);
                }
            }),
        ];
        Self {
            inner,
            buffers,
            callback,
            handlers,
        }
    }
}
impl SourceBufferList for GStreamerSourceBufferList {
    fn length(&self) -> u32 {
        self.inner.length()
    }
    fn index(&self, index: u32) -> MseResult<Option<Box<dyn SourceBuffer>>> {
        self.inner
            .index(index)
            .map(|inner| {
                GStreamerSourceBuffer::wrap(inner, &self.buffers)
                    .map(|buffer| Box::new(buffer) as Box<dyn SourceBuffer>)
            })
            .transpose()
    }
    fn on_event(&self, callback: Box<dyn Fn(SourceBufferListEvent) + Send + Sync>) {
        if let Ok(mut listener) = self.callback.lock() {
            *listener = Some(Arc::from(callback));
        }
    }
    fn clear_event_callback(&self) -> MseResult<()> {
        *self
            .callback
            .lock()
            .map_err(|_| MseError::Backend("SourceBufferList callback lock poisoned".into()))? =
            None;
        Ok(())
    }
}

impl Drop for GStreamerSourceBufferList {
    fn drop(&mut self) {
        for handler in self.handlers.drain(..) {
            self.inner.disconnect(handler);
        }
    }
}
