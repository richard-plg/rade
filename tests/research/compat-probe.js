const c = document.createElement('canvas'), v = document.createElement('video');
let gpu = null;
try { if (navigator.gpu) { const a = await navigator.gpu.requestAdapter(); gpu = a ? 'adapter' : 'no adapter'; } } catch (e) { gpu = 'error ' + e.message; }
let idb = null;
try { idb = await new Promise((res) => { const r = indexedDB.open('rade-probe', 1); r.onupgradeneeded = () => r.result.createObjectStore('s'); r.onsuccess = () => { const t = r.result.transaction('s', 'readwrite'); t.objectStore('s').put(1, 'k'); t.oncomplete = () => res('ok'); t.onerror = () => res('tx error'); }; r.onerror = () => res('open error ' + r.error); setTimeout(() => res('timeout'), 3000); }); } catch (e) { idb = 'throw ' + e.message; }
let sw = null;
try { sw = 'serviceWorker' in navigator ? (await navigator.serviceWorker.getRegistrations()).length + ' regs, controller=' + !!navigator.serviceWorker.controller : 'absent'; } catch (e) { sw = 'error ' + e.message; }
let gum = null;
try { if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) { gum = await Promise.race([navigator.mediaDevices.getUserMedia({video: true}).then(() => 'granted', e => 'denied: ' + e.name), new Promise(r => setTimeout(() => r('no answer in 3s'), 3000))]); } else gum = 'absent'; } catch (e) { gum = 'throw ' + e.message; }
const vids = Array.from(document.querySelectorAll('video')).map(x => ({t: +x.currentTime.toFixed(1), paused: x.paused, err: x.error && x.error.code, rs: x.readyState, src: (x.currentSrc || '').slice(0, 40)}));
return {
  ua: navigator.userAgent, title: document.title, secure: isSecureContext,
  webgl: !!c.getContext('webgl'), webgl2: !!document.createElement('canvas').getContext('webgl2'), webgpu: gpu,
  rtc: typeof RTCPeerConnection, gum, mse: typeof MediaSource, mms: typeof ManagedMediaSource,
  hlsNative: v.canPlayType('application/vnd.apple.mpegurl'), h264: v.canPlayType('video/mp4; codecs="avc1.42E01E"'),
  vp9: v.canPlayType('video/webm; codecs="vp9"'), av1: v.canPlayType('video/mp4; codecs="av01.0.05M.08"'),
  fsa: typeof window.showOpenFilePicker, opfs: !!(navigator.storage && navigator.storage.getDirectory),
  idb, sw, notif: typeof Notification, clip: !!navigator.clipboard, wasm: typeof WebAssembly, sab: typeof SharedArrayBuffer,
  videos: vids, bodyText: (document.body ? document.body.innerText : '').replace(/\s+/g, ' ').slice(0, 160)
};
