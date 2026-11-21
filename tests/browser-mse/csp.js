// Loaded in an isolated same-origin frame with CSP: media-src 'none'.
(async function () {
  const source = new MediaSource(), media = document.createElement('video');
  const result = { opened: false, mediaError: false, violation: false };
  document.addEventListener('securitypolicyviolation', event => {
    if (event.effectiveDirective === 'media-src') result.violation = true;
  });
  source.addEventListener('sourceopen', () => { result.opened = true; });
  media.addEventListener('error', () => { result.mediaError = true; });
  document.body.append(media);
  const url = URL.createObjectURL(source); media.src = url;
  await new Promise(resolve => setTimeout(resolve, 1200));
  media.removeAttribute('src'); media.load(); URL.revokeObjectURL(url);
  window.__mseCspResult = result;
})();
