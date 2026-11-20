'use client';

import React, { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { type RecordingSave, useRecordingStop } from '@/hooks/useRecordingStop';

const noop = () => {};

// The backend queues one save for each successful stop, regardless of source.
export function RecordingPostProcessingProvider({ children }: { children: React.ReactNode }) {
  const { handleRecordingStop } = useRecordingStop(noop, noop);
  const handler = useRef(handleRecordingStop);
  useEffect(() => {
    handler.current = handleRecordingStop;
  }, [handleRecordingStop]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const processPendingSave = async () => {
      if (disposed) return;
      try {
        const recording = await invoke<RecordingSave | null>('claim_pending_recording_save');
        if (!recording) return;
        await handler.current(true, recording);
        await invoke('finish_recording_save', { sessionId: recording.session_id });
      } catch (error) {
        console.error('Recording post-processing failed:', error);
      }
    };

    void listen('recording-stop-complete', () => void processPendingSave())
      .then(stopListening => {
        if (disposed) {
          stopListening();
          return;
        }
        unlisten = stopListening;
        // Pick up a stop that occurred before the webview listener attached.
        void processPendingSave();
      })
      .catch(error => console.error('Failed to listen for a pending recording save:', error));

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return <>{children}</>;
}
