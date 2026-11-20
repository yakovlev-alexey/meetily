'use client';

import React, { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { type StoppedRecording, useRecordingStop } from '@/hooks/useRecordingStop';

const noop = () => {};

/**
 * RecordingPostProcessingProvider
 *
 * This provider handles post-processing when recording stops from any source:
 * - Tray menu stop
 * - Global keyboard shortcut
 * - Overlay stop button
 * - Main UI stop button
 *
 * It listens for the 'recording-stop-complete' event from Rust backend
 * and triggers the full post-processing flow (save to database, navigate, analytics)
 * regardless of which page the user is currently on.
 */
export function RecordingPostProcessingProvider({ children }: { children: React.ReactNode }) {
  // RecordingStateContext already handles the global recording state.
  const { handleRecordingStop } = useRecordingStop(noop, noop);
  const handler = useRef(handleRecordingStop);
  useEffect(() => {
    handler.current = handleRecordingStop;
  }, [handleRecordingStop]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<StoppedRecording>('recording-stop-complete', ({ payload: recording }) => {
      const sessionId = recording.session_id;
      if (disposed) return;
      void invoke<boolean>('begin_recording_post_processing', { sessionId }).then(async claimed => {
        if (!claimed) return;
        try {
          if (!disposed) await handler.current(true, recording);
        } finally {
          await invoke('finish_recording_post_processing', { sessionId });
        }
      }).catch(error => console.error('[RecordingPostProcessing] Failed to finish recording:', error));
    }).then(stopListening => {
      if (disposed) stopListening();
      else unlisten = stopListening;
    }).catch(error => {
      console.error('[RecordingPostProcessing] Failed to set up event listener:', error);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return <>{children}</>;
}
