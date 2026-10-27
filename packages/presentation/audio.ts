export interface AudioBackend {
  createContext: () => AudioContext;
  fetch: (url: string, options: {signal: AbortSignal}) => Promise<{ok: boolean; arrayBuffer: () => Promise<ArrayBuffer>}>;
}
interface Playback {
  controller: AbortController;
  context: AudioContext | null;
  source: AudioBufferSourceNode | null;
  timer: ReturnType<typeof setTimeout>;
  done: (failed: boolean) => void;
}

/** Companion voice clips use Web Audio without registering a media session.
 * Windows output volume/device selection still applies. Completion is an actual
 * source-ended callback, independent of text, animation and environment facts. */
export class TaskAudioPlayer {
  private current: Playback | null = null;
  private backend: AudioBackend;
  constructor(backend: AudioBackend = {
    createContext: () => new AudioContext({latencyHint: 'playback'}),
    fetch: (url, options) => fetch(url, options),
  }) { this.backend = backend; }

  play(url: string, deadlineMs: number, done: (failed: boolean) => void) {
    this.stop();
    const playback: Playback = {
      controller: new AbortController(), context: null, source: null,
      timer: setTimeout(() => this.finish(playback, true), deadlineMs), done,
    };
    this.current = playback;
    void this.prepare(playback, url);
  }
  stop() {
    const playback = this.current;
    this.current = null;
    if (playback) this.release(playback);
  }
  private async prepare(playback: Playback, url: string) {
    try {
      const context = this.backend.createContext();
      playback.context = context;
      const [response] = await Promise.all([
        this.backend.fetch(url, {signal: playback.controller.signal}), context.resume(),
      ]);
      if (this.current !== playback) return;
      if (!response.ok) throw Error('音声读取失败');
      const bytes = await response.arrayBuffer();
      if (this.current !== playback) return;
      const buffer = await context.decodeAudioData(bytes);
      if (this.current !== playback) return;
      const source = context.createBufferSource();
      playback.source = source;
      source.buffer = buffer;
      source.connect(context.destination);
      source.onended = () => this.finish(playback, false);
      source.start();
    } catch {
      this.finish(playback, true);
    }
  }
  private finish(playback: Playback, failed: boolean) {
    if (this.current !== playback) return;
    this.current = null;
    this.release(playback);
    playback.done(failed);
  }
  private release(playback: Playback) {
    clearTimeout(playback.timer);
    playback.controller.abort();
    if (playback.source) {
      playback.source.onended = null;
      try { playback.source.stop(); } catch { /* Already ended or not yet started. */ }
      playback.source.disconnect();
      playback.source = null;
    }
    if (playback.context) {
      void playback.context.close().catch(() => {});
      playback.context = null;
    }
  }
}
