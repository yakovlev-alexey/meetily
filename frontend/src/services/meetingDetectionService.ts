/**
 * Meeting Detection Service
 *
 * Handles meeting-provider discovery, settings, and live detector status.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';

export type MeetingPlatform = 'macos' | 'windows' | 'linux' | 'unknown';

export interface MeetingDetectionSettings {
  enabled: boolean;
  auto_start: boolean;
  auto_stop: boolean;
  enabled_providers: Record<string, boolean>;
}

export interface MeetingProviderDescriptor {
  id: string;
  display_name: string;
  supported: boolean;
  supported_platforms: MeetingPlatform[];
}

export type MeetingDetectionPhase =
  | 'disabled'
  | 'idle'
  | 'confirming'
  | 'auto_starting'
  | 'auto_recording'
  | 'grace_period'
  | 'auto_stopping'
  | 'suppressed'
  | 'stop_failed';

export type MeetingDetectionSuppressionReason =
  | 'manual_recording'
  | 'manual_stop'
  | 'start_failed';

export interface MeetingDetectionCoordinatorStatus {
  phase: MeetingDetectionPhase;
  provider_id: string | null;
  detector_owns_recording: boolean;
  suppression_reason: MeetingDetectionSuppressionReason | null;
}

export interface MeetingDetectionStatus {
  enabled: boolean;
  detected_provider_id: string | null;
  detected_provider_name: string | null;
  coordinator: MeetingDetectionCoordinatorStatus;
}

export class MeetingDetectionService {
  async getSettings(): Promise<MeetingDetectionSettings> {
    return invoke<MeetingDetectionSettings>('get_meeting_detection_settings');
  }

  async setSettings(settings: MeetingDetectionSettings): Promise<void> {
    return invoke('set_meeting_detection_settings', { settings });
  }

  async getStatus(): Promise<MeetingDetectionStatus> {
    return invoke<MeetingDetectionStatus>('get_meeting_detection_status');
  }

  async listProviders(): Promise<MeetingProviderDescriptor[]> {
    return invoke<MeetingProviderDescriptor[]>('list_meeting_detection_providers');
  }

  async onStatusChanged(
    callback: (status: MeetingDetectionStatus) => void
  ): Promise<UnlistenFn> {
    return listen<MeetingDetectionStatus>('meeting-detection-status-changed', (event) => {
      callback(event.payload);
    });
  }
}

export const meetingDetectionService = new MeetingDetectionService();
