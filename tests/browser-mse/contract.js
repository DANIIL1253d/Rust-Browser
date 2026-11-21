// Isolated real-engine contract. Native stubs cannot satisfy these cases.
(async function () {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const timeout = (promise, name, ms = 10000) => {
    const result = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('Timed out: ' + name)), ms);
      promise.then(value => { clearTimeout(timer); resolve(value); }, error => { clearTimeout(timer); reject(error); });
    });
    result.catch(() => {}); // A synchronous operation rejection can leave its watcher unused.
    return result;
  };
  const wait = (target, name) => timeout(new Promise(resolve => target.addEventListener(name, resolve, { once: true })), name);
  const throws = (fn, name) => {
    try { fn(); } catch (error) { assert(error.name === name, `Expected ${name}, got ${error.name}`); return; }
    throw new Error('Missing ' + name);
  };
  const selected = new URLSearchParams(location.search).get('case');
  const passed = [], errors = [];
  let samples, stage;
  async function data() {
    return samples ||= Promise.all(['/video.webm', '/audio.webm'].map(url => fetch(url).then(response => {
      assert(response.ok, 'Missing fixture: ' + url); return response.arrayBuffer();
    })));
  }
  async function attached(test) {
    const source = new MediaSource(), media = document.createElement('video');
    media.muted = true; document.body.append(media);
    const url = URL.createObjectURL(source), opened = wait(source, 'sourceopen');
    media.src = url;
    try { await opened; assert(source.readyState === 'open', 'sourceopen state'); await test(source, media); }
    finally { media.pause(); media.removeAttribute('src'); media.load(); URL.revokeObjectURL(url); media.remove(); }
  }
  async function append(buffer, bytes) {
    const events = [], handlers = [];
    for (const name of ['updatestart', 'update', 'updateend', 'error']) {
      const listener = () => events.push(name); handlers.push([name, listener]); buffer.addEventListener(name, listener);
    }
    try {
      const ended = wait(buffer, 'updateend'); buffer.appendBuffer(bytes);
      assert(buffer.updating, 'append must set updating synchronously');
      throws(() => buffer.appendBuffer(bytes), 'InvalidStateError');
      await ended; assert(!buffer.updating, 'updateend must clear updating');
      assert(events.join(',') === 'updatestart,update,updateend', 'Append events: ' + events);
    } finally { for (const [name, listener] of handlers) buffer.removeEventListener(name, listener); }
  }
  async function av(source) {
    stage = 'add-video-buffer';
    const video = source.addSourceBuffer('video/webm; codecs="vp8"');
    stage = 'add-audio-buffer';
    const audio = source.addSourceBuffer('audio/webm; codecs="opus"');
    assert(source.sourceBuffers.length === 2 && video !== audio, 'Independent A/V SourceBuffers');
    stage = 'fetch-fixtures';
    const [v, a] = await data(); stage = 'append-av'; await Promise.all([append(video, v), append(audio, a)]);
    stage = 'appended-av';
    return { video, audio, v, a };
  }
  async function run(name, test) {
    if (selected && selected !== name) return;
    window.__mseProgress = { current: name, passed: [...passed], errors: [...errors] };
    stage = name;
    try { await test(); passed.push(name); } catch (error) { errors.push({ name, stage, error: String(error), stack: error.stack }); }
  }
  try {
    assert(typeof MediaSource === 'function', 'MediaSource is not enabled');
    await run('closed-state', async () => {
      const source = new MediaSource();
      assert(source.readyState === 'closed' && Number.isNaN(source.duration), 'Closed state/duration');
      assert(!MediaSource.isTypeSupported('not/a-media-type; codecs="invalid"'), 'Unsupported MIME advertised');
      throws(() => source.addSourceBuffer(''), 'TypeError');
      throws(() => source.endOfStream(), 'InvalidStateError');
      throws(() => { source.duration = 1; }, 'InvalidStateError');
    });
    await run('csp', async () => {
      const frame = document.createElement('iframe'); frame.src = '/csp'; document.body.append(frame);
      try {
        const result = await timeout((async () => {
          while (!frame.contentWindow.__mseCspResult) await new Promise(resolve => setTimeout(resolve, 50));
          return frame.contentWindow.__mseCspResult;
        })(), 'CSP result');
        assert(!result.opened && result.mediaError && result.violation,
          'CSP media-src none must block before sourceopen and report: ' + JSON.stringify(result));
      } finally { frame.remove(); }
    });
    await run('append-av', () => attached(async source => {
      assert(MediaSource.isTypeSupported('video/webm; codecs="vp8"'), 'VP8 unavailable');
      assert(MediaSource.isTypeSupported('audio/webm; codecs="opus"'), 'Opus unavailable');
      const oversized = source.addSourceBuffer('video/webm; codecs="vp8"');
      throws(() => oversized.appendBuffer(new Uint8Array(33 * 1024 * 1024)), 'QuotaExceededError');
      assert(!oversized.updating, 'Oversized rejection must leave updating false');
      source.removeSourceBuffer(oversized);
      const { video, audio } = await av(source);
      assert(video.buffered.length > 0 && audio.buffered.length > 0, 'Native parsed ranges missing');
      assert(source.activeSourceBuffers.length === 2, 'Both parsed SourceBuffers active');
    }));
    await run('playback-seek', () => attached(async (source, media) => {
      await av(source); stage = 'end-of-stream'; source.endOfStream(); assert(source.readyState === 'ended', 'EOS state');
      stage = 'play';
      await timeout(media.play(), 'play promise');
      stage = 'playback-progress';
      const end = performance.now() + 6000;
      while (media.currentTime < 0.1 && performance.now() < end) await new Promise(resolve => setTimeout(resolve, 50));
      assert(media.currentTime >= 0.1, 'No decoded playback progress');
      stage = 'seek';
      const seeked = wait(media, 'seeked'); media.currentTime = 0.4; await seeked; media.pause();
      assert(Math.abs(media.currentTime - 0.4) < 0.3, 'Seek did not reach buffered time');
    }));
    await run('detach', () => attached(async (source, media) => {
      const video = source.addSourceBuffer('video/webm; codecs="vp8"'), [v] = await data();
      const closed = wait(source, 'sourceclose');
      video.appendBuffer(v); media.removeAttribute('src'); media.load(); await closed;
      assert(source.readyState === 'closed' && source.sourceBuffers.length === 0, 'Detach must close and empty lists');
      throws(() => video.appendBuffer(v), 'InvalidStateError');
      assert(!video.updating, 'Removed SourceBuffer still updating');
      const opened = wait(source, 'sourceopen'), again = URL.createObjectURL(source);
      try { media.src = again; await opened; assert(source.sourceBuffers.length === 0, 'Reattach retained stale buffers'); }
      finally { URL.revokeObjectURL(again); }
    }));
    await run('timestamp-offset', () => attached(async (source, media) => {
      const video = source.addSourceBuffer('video/webm; codecs="vp8"'), [v] = await data();
      video.timestampOffset = 4; await append(video, v);
      source.endOfStream(); media.currentTime = 4;
      await timeout(media.play(), 'offset playback');
      const end = performance.now() + 6000;
      while (!video.buffered.length && performance.now() < end) await new Promise(resolve => setTimeout(resolve, 50));
      media.pause();
      assert(video.buffered.length > 0 && video.buffered.start(0) >= 3.9, 'Offset not applied to native frames');
      video.timestampOffset = 5;
      assert(source.readyState === 'open', 'Offset must reopen ended source');
    }));
    await run('reattach-events', () => attached(async (source, media) => {
      const events = [];
      source.addEventListener('sourceclose', () => events.push('sourceclose'), { once: true });
      source.addEventListener('sourceopen', () => events.push('sourceopen'), { once: true });
      const closed = wait(source, 'sourceclose'), opened = wait(source, 'sourceopen');
      // Same MediaSource, new HTML load generation, without waiting for the old
      // close event to dispatch before the stable-state attachment task runs.
      media.load(); await Promise.all([closed, opened]);
      assert(events.join(',') === 'sourceclose,sourceopen', 'Reattach dropped queued events: ' + events);
    }));
    await run('malformed-data', () => attached(async source => {
      const buffer = source.addSourceBuffer('video/webm; codecs="vp8"'), events = [];
      for (const name of ['error', 'updateend']) buffer.addEventListener(name, () => events.push(name), { once: true });
      const ended = wait(buffer, 'updateend'); buffer.appendBuffer(new Uint8Array([0xff, 1, 2, 3, 4, 5, 6, 7])); await ended;
      assert(events.join(',') === 'error,updateend' && !buffer.updating, 'Malformed append events: ' + events);
    }));
    await run('remove', () => attached(async (source, media) => {
      const { video } = await av(source); throws(() => video.remove(-1, 1), 'TypeError');
      source.endOfStream(); await timeout(media.play(), 'playback before removal');
      const end = performance.now() + 6000;
      while (!video.buffered.length && performance.now() < end) await new Promise(resolve => setTimeout(resolve, 50));
      media.pause();
      assert(video.buffered.length > 0, 'No native frames to remove');
      const before = video.buffered.start(0), removed = wait(video, 'updateend');
      video.remove(0, 0.8); assert(video.updating, 'remove updating'); await removed;
      assert(video.buffered.length === 0 || video.buffered.start(0) > before, 'remove retained native frames');
    }));
    await run('abort', () => attached(async source => {
      const video = source.addSourceBuffer('video/webm; codecs="vp8"'), [v] = await data(), events = [];
      for (const name of ['abort', 'updateend']) video.addEventListener(name, () => events.push(name), { once: true });
      const ended = wait(video, 'updateend'); video.appendBuffer(v); video.abort(); await ended;
      assert(events.join(',') === 'abort,updateend' && !video.updating, 'Abort did not cancel native append');
    }));
    assert(passed.length + errors.length > 0, 'Unknown test case: ' + selected);
    window.__mseResult = { passed, errors, ...(errors.length ? { error: errors.length + ' MSE contract cases failed' } : {}) };
  } catch (error) { window.__mseResult = { passed, errors, error: String(error), stack: error.stack }; }
})();
