import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import ts from 'typescript';

function loadSource(path, imports, globals = {}) {
  const file = new URL(`../../src/${path}`, import.meta.url);
  const code = ts.transpileModule(readFileSync(file, 'utf8'), {
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
    },
  }).outputText;
  const exported = {};
  vm.runInNewContext(code, {
    exports: exported,
    require: name => {
      assert.ok(name in imports, `Unexpected test dependency: ${name}`);
      return imports[name];
    },
    ...globals,
  });
  return exported;
}

for (const signal of ['permission_required', 'unavailable']) {
  test(`an initial status read cannot overwrite a newer ${signal} event`, async () => {
    const observed = [];
    let notify;
    let completeSnapshot;
    let startSnapshot;
    let unlistened = false;
    const snapshotStarted = new Promise(resolve => { startSnapshot = resolve; });
    const { meetingDetectionService } = loadSource('services/meetingDetectionService.ts', {
      '@tauri-apps/api/core': {
        invoke: () => {
          assert.equal(typeof notify, 'function', 'the listener is ready before reading');
          return new Promise(resolve => {
            completeSnapshot = resolve;
            startSnapshot();
          });
        },
      },
      '@tauri-apps/api/event': {
        listen: async (_name, callback) => {
          notify = callback;
          return () => { unlistened = true; };
        },
      },
    });

    const subscription = meetingDetectionService.watchStatus(status => observed.push(status.telemost_signal));
    await snapshotStarted;
    notify({ payload: { telemost_signal: signal } });
    completeSnapshot({ telemost_signal: 'active' });
    const unlisten = await subscription;
    assert.deepEqual(observed, [signal]);
    unlisten();
    assert.ok(unlistened);
  });
}

test('the stop listener only saves once after the backend grants ownership', async () => {
  const finished = [];
  let handlerCalls = 0;
  let notifyStop;
  const { RecordingPostProcessingProvider } = loadSource('contexts/RecordingPostProcessingProvider.tsx', {
    react: {
      useEffect: effect => { effect(); },
      useRef: current => ({ current }),
    },
    'react/jsx-runtime': { jsx: () => null },
    '@tauri-apps/api/event': {
      listen: async (_name, callback) => {
        notifyStop = callback;
        return () => {};
      },
    },
    '@tauri-apps/api/core': {
      invoke: async (command, { sessionId }) => {
        if (command === 'begin_recording_post_processing') {
          if (sessionId !== 3 || handlerCalls !== 0) return false;
          return true;
        }
        assert.equal(command, 'finish_recording_post_processing');
        finished.push(sessionId);
      },
    },
    '@/hooks/useRecordingStop': {
      useRecordingStop: () => ({
        handleRecordingStop: async (shouldSave, stoppedRecording) => {
          assert.equal(shouldSave, true);
          assert.equal(stoppedRecording.folder_path, '/saved-call');
          assert.equal(stoppedRecording.meeting_name, 'Saved call');
          handlerCalls += 1;
        },
      }),
    },
  });
  RecordingPostProcessingProvider({});

  const recording = { session_id: 3, folder_path: '/saved-call', meeting_name: 'Saved call' };
  notifyStop({ payload: { ...recording, session_id: 2 } });
  notifyStop({ payload: recording });
  await new Promise(setImmediate);
  notifyStop({ payload: recording });
  await new Promise(setImmediate);

  assert.equal(handlerCalls, 1);
  assert.deepEqual(finished, [3]);
});

for (const [description, statusReadFails] of [
  ['a failed SQLite save leaves the live transcript available when there is no backup', false],
  ['a completed stop is saved even if a redundant status read would fail', true],
]) {
  test(description, async () => {
    const noop = () => {};
    let cleared = false;
    const saved = [];
    const { useRecordingStop } = loadSource('hooks/useRecordingStop.ts', {
      react: { useCallback: callback => callback, useEffect: noop, useRef: current => ({ current }) },
      'next/navigation': { useRouter: () => ({}) },
      '@tauri-apps/api/event': { listen: async () => noop },
      sonner: { toast: { error: noop } },
      '@/contexts/TranscriptContext': {
        useTranscripts: () => ({
          transcriptsRef: { current: [{ text: 'Previous meeting' }] },
          flushBuffer: noop,
          clearTranscripts: () => { cleared = true; },
          markMeetingAsSaved: assert.fail,
        }),
      },
      '@/components/Sidebar/SidebarProvider': { useSidebar: () => ({}) },
      '@/contexts/RecordingStateContext': { useRecordingState: () => ({ setStatus: noop }), RecordingStatus: {} },
      '@/services/storageService': {
        storageService: { saveMeeting: async (title, _transcripts, folder) => {
          saved.push({ title, folder });
          throw new Error('SQLite unavailable');
        } },
      },
      '@/services/transcriptService': {
        transcriptService: { getTranscriptionStatus: async () => {
          if (statusReadFails) throw new Error('Transcription status unavailable');
          return { is_processing: false, chunks_in_queue: 0 };
        } },
      },
      '@/lib/analytics': {},
      '@/lib/summary-language-preferences': {},
    }, {
      console: { log: noop, warn: noop, error: noop },
      setTimeout: callback => callback(),
      sessionStorage: { getItem: () => null },
    });

    await useRecordingStop(noop, noop).handleRecordingStop(true, {
      session_id: 1,
      folder_path: '/saved-call',
      meeting_name: 'Saved call',
    });
    assert.deepEqual(saved, [{ title: 'Saved call', folder: '/saved-call' }]);
    assert.equal(cleared, false);
  });
}

test('a new recording clears the previous transcript independently of the backup store', async () => {
  const noop = () => {};
  const previousTranscript = [{ text: 'Unsaved meeting' }];
  let shown = previousTranscript;
  let notifyStart;
  const { TranscriptProvider } = loadSource('contexts/TranscriptContext.tsx', {
    react: {
      createContext: () => ({ Provider: 'transcript-provider' }),
      useState: value => Array.isArray(value) ? [previousTranscript, update => { shown = update; }] : [value, noop],
      useEffect: effect => { effect(); },
      useCallback: callback => callback,
      useRef: current => ({ current }),
    },
    'react/jsx-runtime': { jsx: (_type, { value }) => value },
    sonner: {},
    './RecordingStateContext': { useRecordingState: () => ({ isRecording: false }) },
    '@/services/transcriptService': { transcriptService: { onTranscriptUpdate: async () => noop } },
    '@/services/recordingService': {
      recordingService: {
        onRecordingStarted: async callback => { notifyStart = callback; return noop; },
        onRecordingStopped: async () => noop,
        getRecordingMeetingName: async () => 'New meeting',
      },
    },
    '@/services/indexedDBService': {
      indexedDBService: {
        init: async () => { throw new Error('Backup store unavailable'); },
        saveMeetingMetadata: async () => { throw new Error('Backup store unavailable'); },
      },
    },
  }, {
    console: { log: noop, error: noop },
    sessionStorage: { setItem: noop },
    clearTimeout,
  });

  const transcript = TranscriptProvider({});
  assert.equal(shown, previousTranscript);
  assert.equal(typeof notifyStart, 'function');
  await notifyStart();
  assert.equal(shown.length, 0);
  assert.equal(transcript.transcriptsRef.current.length, 0);
});

test('late transcript history cannot reappear after a new recording starts', async () => {
  const noop = () => {};
  let shown = [];
  let notifyStart;
  let resolveHistory;
  const { TranscriptProvider } = loadSource('contexts/TranscriptContext.tsx', {
    react: {
      createContext: () => ({ Provider: 'transcript-provider' }),
      useState: value => Array.isArray(value)
        ? [shown, update => { shown = typeof update === 'function' ? update(shown) : update; }]
        : [value, noop],
      useEffect: effect => { effect(); },
      useCallback: callback => callback,
      useRef: current => ({ current }),
    },
    'react/jsx-runtime': { jsx: noop },
    sonner: {},
    './RecordingStateContext': { useRecordingState: () => ({ isRecording: true }) },
    '@/services/transcriptService': {
      transcriptService: {
        onTranscriptUpdate: async () => noop,
        getTranscriptHistory: () => new Promise(resolve => { resolveHistory = resolve; }),
      },
    },
    '@/services/recordingService': {
      recordingService: {
        onRecordingStarted: async callback => { notifyStart = callback; return noop; },
        onRecordingStopped: async () => noop,
        getRecordingMeetingName: async () => 'New meeting',
      },
    },
    '@/services/indexedDBService': {
      indexedDBService: { saveMeetingMetadata: async () => { throw new Error('No backup'); } },
    },
  }, {
    console: { log: noop, error: noop },
    sessionStorage: { setItem: noop },
    clearTimeout,
  });

  TranscriptProvider({});
  await notifyStart();
  resolveHistory([{ id: 'old', sequence_id: 1, text: 'Previous call' }]);
  await new Promise(setImmediate);
  assert.equal(shown.length, 0);
});

test('a stop without post-processing leaves recovery available after reloading', async () => {
  const noop = () => {};
  const unlisten = async () => noop;
  let state;
  let notifyStop;
  const { RecordingStateProvider, RecordingStatus } = loadSource('contexts/RecordingStateContext.tsx', {
    react: {
      createContext: () => ({ Provider: 'recording-state' }),
      useState: initial => {
        state = initial;
        return [state, update => { state = typeof update === 'function' ? update(state) : update; }];
      },
      useEffect: effect => { effect(); },
      useRef: current => ({ current }),
      useCallback: callback => callback,
      useMemo: run => run(),
    },
    'react/jsx-runtime': { jsx: noop },
    '@/services/recordingService': {
      recordingService: {
        onRecordingStarted: unlisten,
        onRecordingStopped: async callback => { notifyStop = callback; return noop; },
        onRecordingPaused: unlisten,
        onRecordingResumed: unlisten,
        getRecordingState: async () => ({ is_recording: true }),
      },
    },
  }, { console: { log: noop, error: noop } });

  RecordingStateProvider({});
  await new Promise(setImmediate);
  notifyStop({ will_save: false });
  assert.equal(state.status, RecordingStatus.IDLE);
  assert.equal(state.isRecording, false);
});
