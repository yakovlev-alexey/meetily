'use client';

import React, { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { type RecordingSave, useRecordingStop } from '@/hooks/useRecordingStop';

const noop = () => {};

let consumerRegistration: Promise<number> | undefined;

function registerConsumer() {
  consumerRegistration ??= invoke<number>('register_recording_save_consumer').catch(error => {
    consumerRegistration = undefined;
    throw error;
  });
  return consumerRegistration;
}

// The backend queues one save for each successful stop, regardless of source.
export function RecordingPostProcessingProvider({ children }: { children: React.ReactNode }) {
  const { handleRecordingStop } = useRecordingStop(noop, noop);
  const handler = useRef(handleRecordingStop);
  const processing = useRef(false);
  useEffect(() => {
    handler.current = handleRecordingStop;
  }, [handleRecordingStop]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;

    const scheduleRetry = () => {
      retry ??= setTimeout(() => {
        retry = undefined;
        void processPendingSave();
      }, 1000);
    };

    const processPendingSave = async () => {
      if (disposed) return;
      if (processing.current) {
        scheduleRetry();
        return;
      }

      processing.current = true;
      try {
        const consumerId = await registerConsumer();
        if (disposed) return;
        const recording = await invoke<RecordingSave | null>('claim_pending_recording_save', { consumerId });
        if (!recording) return;
        await handler.current(true, recording);
        await invoke('finish_recording_save', { sessionId: recording.session_id, consumerId });
      } catch (error) {
        console.error('Recording post-processing failed:', error);
        if (!disposed) {
          scheduleRetry();
        }
      } finally {
        processing.current = false;
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
      if (retry) clearTimeout(retry);
    };
  }, []);

  return <>{children}</>;
}
