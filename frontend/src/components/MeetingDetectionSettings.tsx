import React, { useEffect, useState } from 'react';
import { CircleDot, ShieldCheck, Video } from 'lucide-react';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import {
  MeetingDetectionSettings as MeetingDetectionSettingsValue,
  MeetingDetectionStatus,
  MeetingProviderDescriptor,
  meetingDetectionService
} from '@/services/meetingDetectionService';

const PHASE_LABELS: Record<MeetingDetectionStatus['coordinator']['phase'], string> = {
  disabled: 'Disabled',
  idle: 'Waiting for a meeting',
  confirming: 'Confirming meeting activity',
  auto_starting: 'Starting recording',
  auto_recording: 'Recording automatically',
  grace_period: 'Waiting for the meeting to reconnect',
  auto_stopping: 'Stopping recording',
  suppressed: 'Automatic action paused',
  stop_failed: 'Automatic stop failed'
};

function supportedPlatforms(provider: MeetingProviderDescriptor): string {
  return provider.supported_platforms
    .map(platform => platform === 'macos' ? 'macOS' : platform[0].toUpperCase() + platform.slice(1))
    .join(', ');
}

export function MeetingDetectionSettings() {
  const [settings, setSettings] = useState<MeetingDetectionSettingsValue | null>(null);
  const [providers, setProviders] = useState<MeetingProviderDescriptor[]>([]);
  const [status, setStatus] = useState<MeetingDetectionStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;

    const load = async () => {
      try {
        const [savedSettings, availableProviders, currentStatus] = await Promise.all([
          meetingDetectionService.getSettings(),
          meetingDetectionService.listProviders(),
          meetingDetectionService.getStatus()
        ]);

        if (!mounted) return;
        setSettings(savedSettings);
        setProviders(availableProviders);
        setStatus(currentStatus);

        const stopListening = await meetingDetectionService.onStatusChanged(nextStatus => {
          if (mounted) setStatus(nextStatus);
        });
        if (mounted) {
          unlisten = stopListening;
        } else {
          stopListening();
        }
      } catch (error) {
        console.error('Failed to load meeting detection settings:', error);
        toast.error('Failed to load meeting automation settings');
      } finally {
        if (mounted) setLoading(false);
      }
    };

    load();

    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  const save = async (nextSettings: MeetingDetectionSettingsValue) => {
    if (!settings) return;

    const previousSettings = settings;
    setSettings(nextSettings);
    setSaving(true);

    try {
      await meetingDetectionService.setSettings(nextSettings);
      toast.success('Meeting automation preference saved');
    } catch (error) {
      setSettings(previousSettings);
      console.error('Failed to save meeting detection settings:', error);
      toast.error('Failed to save meeting automation preference', {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="animate-pulse space-y-4">
        <div className="h-5 bg-gray-200 rounded w-1/3" />
        <div className="h-24 bg-gray-200 rounded" />
        <div className="h-24 bg-gray-200 rounded" />
      </div>
    );
  }

  if (!settings) {
    return (
      <div className="p-4 border border-red-200 rounded-lg bg-red-50 text-sm text-red-800">
        Meeting automation settings are unavailable. Restart Meetily and try again.
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold mb-4">Meeting Automation</h3>
        <p className="text-sm text-gray-600 mb-6">
          Detect supported meeting apps locally and control Meetily recording automatically.
          No meeting account or cloud integration is required.
        </p>
      </div>

      <div className="flex items-center justify-between gap-6 p-4 border rounded-lg">
        <div className="flex items-start gap-3">
          <CircleDot className="w-5 h-5 mt-0.5 text-blue-600" />
          <div>
            <div className="font-medium">Automatically detect meetings</div>
            <div className="text-sm text-gray-600">
              Monitor local meeting-app activity while Meetily is running
            </div>
          </div>
        </div>
        <Switch
          checked={settings.enabled}
          onCheckedChange={enabled => save({ ...settings, enabled })}
          disabled={saving}
        />
      </div>

      <div className={!settings.enabled ? 'space-y-4 opacity-60' : 'space-y-4'}>
        <div className="flex items-center justify-between gap-6 p-4 border rounded-lg">
          <div>
            <div className="font-medium">Start recording when a meeting begins</div>
            <div className="text-sm text-gray-600">
              Wait for a stable meeting signal before starting
            </div>
          </div>
          <Switch
            checked={settings.auto_start}
            onCheckedChange={auto_start => save({ ...settings, auto_start })}
            disabled={saving || !settings.enabled}
          />
        </div>

        <div className="flex items-center justify-between gap-6 p-4 border rounded-lg">
          <div>
            <div className="font-medium">Stop recording when the meeting ends</div>
            <div className="text-sm text-gray-600">
              Applies only to recordings started by meeting automation
            </div>
          </div>
          <Switch
            checked={settings.auto_stop}
            onCheckedChange={auto_stop => save({ ...settings, auto_stop })}
            disabled={saving || !settings.enabled}
          />
        </div>
      </div>

      <div className="space-y-3 border-t pt-6">
        <div>
          <h4 className="text-base font-medium text-gray-900">Meeting apps</h4>
          <p className="text-sm text-gray-600 mt-1">
            Support is supplied by detector providers, so more apps and platforms can be added later.
          </p>
        </div>

        {providers.map(provider => {
          const enabled = settings.enabled_providers[provider.id] ?? false;

          return (
            <div
              key={provider.id}
              className="flex items-center justify-between gap-6 p-4 border rounded-lg bg-gray-50"
            >
              <div className="flex items-center gap-3">
                <Video className="w-5 h-5 text-gray-700" />
                <div>
                  <div className="font-medium">{provider.display_name}</div>
                  <div className="text-sm text-gray-600">
                    {provider.supported
                      ? `Supported on this device (${supportedPlatforms(provider)})`
                      : `Available on ${supportedPlatforms(provider)}`}
                  </div>
                </div>
              </div>
              <Switch
                checked={enabled}
                onCheckedChange={providerEnabled => save({
                  ...settings,
                  enabled_providers: {
                    ...settings.enabled_providers,
                    [provider.id]: providerEnabled
                  }
                })}
                disabled={saving || !provider.supported || !settings.enabled}
              />
            </div>
          );
        })}
      </div>

      {settings.enabled && status && (
        <div className="p-4 border border-blue-200 rounded-lg bg-blue-50">
          <div className="text-sm font-medium text-blue-900">
            Status: {PHASE_LABELS[status.coordinator.phase]}
          </div>
          {status.detected_provider_name && (
            <div className="text-xs text-blue-700 mt-1">
              Detected app: {status.detected_provider_name}
            </div>
          )}
        </div>
      )}

      <div className="flex gap-3 p-4 border border-yellow-200 rounded-lg bg-yellow-50">
        <ShieldCheck className="w-5 h-5 shrink-0 text-yellow-700" />
        <div className="text-sm text-yellow-900">
          Automatic recording does not replace consent requirements. Inform participants and follow
          the laws and policies that apply to your meetings.
        </div>
      </div>
    </div>
  );
}
