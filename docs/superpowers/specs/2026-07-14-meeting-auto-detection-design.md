# Meeting Auto-Detection Design

## Summary

Meetily will gain an opt-in, local meeting auto-detection feature. The first
provider detects active Zoom meetings on macOS and can automatically start and
stop Meetily recording. The implementation separates application-specific
detection from recording lifecycle coordination so future providers such as
Microsoft Teams, Google Meet, and platform-specific detectors can be added
without changing the recording state machine.

The feature preserves Meetily's local-first behavior. It does not use Zoom
OAuth, cloud APIs, meeting bots, or meeting content. It inspects local process
metadata only until Meetily starts its existing system-audio and microphone
capture flow.

## Goals

- Detect entry into and exit from an active Zoom meeting on macOS.
- Automatically start recording after a stable positive detection.
- Automatically stop and save only recordings started by the detector.
- Keep manual recording behavior unchanged and outside detector ownership.
- Make detector configuration persistent and opt-in.
- Expose provider metadata to the frontend so additional meeting applications
  can be added without restructuring the settings page.
- Cover provider behavior and lifecycle transitions with deterministic Rust
  unit tests.

## Non-Goals

- Joining meetings or controlling the Zoom client.
- Zoom OAuth, REST API, Meeting SDK, or cloud webhook integration.
- Calendar integration or scheduled recording.
- Identifying meeting titles, IDs, participants, or content.
- Implementing Teams, Google Meet, Windows, or Linux detection in the first
  version.
- Retrying a failed automatic start indefinitely while the same meeting remains
  active.

## User Experience

Add an `Automation` tab to the existing settings page between `Recordings` and
`Transcription`.

```text
Settings
 General | Recordings | Automation | Transcription | Summary | Beta

 Automation
 +---------------------------------------------------------+
 | Automatically detect meetings                    [on]  |
 | Start recording when a meeting begins             [on]  |
 | Stop detector-owned recording when it ends        [on]  |
 +---------------------------------------------------------+

 Applications
 +---------------------------------------------------------+
 | Zoom                         macOS supported       [on]  |
 +---------------------------------------------------------+
```

The master switch is off by default. When the user enables it, automatic start
and stop are enabled by default. Provider toggles are rendered from descriptors
returned by the Rust backend rather than from a hard-coded frontend list.

The existing recording-started reminder remains the user-facing consent
reminder. Existing recording start and stop notifications continue to respect
the user's notification preferences. Detector failures use the existing system
error notification path and also emit a frontend event for logging and future
UI treatment.

## Architecture

Create a new Rust module at `frontend/src-tauri/src/meeting_detection`:

```text
meeting_detection/
|-- mod.rs
|-- context.rs
|-- coordinator.rs
|-- commands.rs
|-- monitor.rs
|-- provider.rs
|-- settings.rs
`-- providers/
    |-- mod.rs
    `-- zoom.rs
```

### Detection Context

`DetectionContext` is a snapshot collected once per polling cycle and shared by
all enabled providers. Its first implementation contains normalized process
names. The type is intentionally owned by the detection module so it can later
include window metadata or active audio sessions without changing coordinator
behavior.

The monitor uses the existing `sysinfo` dependency. It performs one process
refresh per cycle regardless of the number of providers.

### Provider Interface

Each meeting application implements a pure `MeetingProvider` interface:

- stable provider ID;
- display name;
- supported platforms;
- detection against `DetectionContext`;
- optional evidence describing why the meeting is considered active.

Provider output contains the provider ID and non-sensitive evidence kind. It
does not contain window titles, meeting IDs, participant information, or audio
content.

The initial Zoom provider reports an active meeting on macOS when the Zoom
`CptHost` helper process is present. The ordinary Zoom application process is
not sufficient because it remains running outside meetings.

### Monitor

One background monitor is initialized as managed Tauri state during application
setup. It loads settings from the Tauri store and owns a cancellation-safe Tokio
task. Settings updates are applied to the running monitor without creating
duplicate polling tasks.

Default timing:

- polling interval: 2 seconds;
- positive confirmations before start: 2;
- stop grace period after the last positive observation: 5 seconds.

The timing values are internal constants in the first version. They are kept in
the domain configuration so they can become advanced settings later without
changing the state machine.

### Coordinator and Ownership

The coordinator is independent of process inspection. It receives observations
and the current Meetily recording state, then produces lifecycle actions.

```text
Disabled
   |
   v
Idle -- stable detection --> AutoStarting --> AutoRecording
 ^                               |                  |
 |                               v                  v
 +-- meeting absent --------- Suppressed <-- manual stop/failure
                                                |
                                                v
                                          meeting absent

AutoRecording -- signal lost --> GracePeriod -- timeout --> AutoStopping --> Idle
                        ^             |
                        +-- signal returns
```

Rules:

1. An already-running manual recording is observed but never claimed.
2. Only a successful detector-initiated start creates detector ownership.
3. Only a detector-owned recording may be auto-stopped.
4. If the user manually stops a detector-owned recording while the provider is
   still active, the coordinator suppresses restart until that meeting signal
   disappears.
5. A failed automatic start is reported once and suppressed until the current
   meeting signal disappears.
6. A short Zoom reconnect or helper-process restart is absorbed by the stop
   grace period.
7. Disabling detection while an owned recording is active does not stop the
   recording. It relinquishes automation and leaves control to the user.

### Recording Integration

The coordinator invokes the existing Rust recording lifecycle, not frontend
button handlers. Automatic start uses the user's configured microphone, system
audio device, save preference, and transcription model. The generated meeting
name follows `Zoom Meeting YYYY-MM-DD_HH-MM-SS`.

Automatic stop must use the same graceful stop path as tray-driven stop and emit
`recording-stop-complete` after the audio/transcription shutdown succeeds. This
preserves the global frontend post-processing flow that saves transcripts and
meeting metadata to SQLite even when the window is hidden or showing another
page.

Lifecycle helpers shared by tray and meeting detection should be extracted only
where required to prevent divergent save-path or post-processing behavior. The
audio pipeline itself is not modified.

## Settings and Tauri API

Persist a version-tolerant `MeetingDetectionSettings` value in the Tauri plugin
store. Missing fields use serde defaults.

```text
enabled: false
auto_start: true
auto_stop: true
enabled_providers:
  zoom: true
```

Expose these Tauri commands:

- `get_meeting_detection_settings`
- `set_meeting_detection_settings`
- `get_meeting_detection_status`
- `list_meeting_detection_providers`

Status includes whether the monitor is enabled, the currently detected provider,
whether the detector owns the active recording, and the coordinator state. It
contains no meeting content.

Emit these events:

- `meeting-detection-status-changed`
- `meeting-detection-error`

The frontend uses a focused `meetingDetectionService` wrapper for the commands
and a `MeetingDetectionSettings` component for the new tab. No new global React
state container is required for the settings page.

## Error Handling

- Settings load failure falls back to disabled defaults and logs the error.
- An unavailable or unsupported provider is returned with `supported: false`
  and cannot be enabled by the UI.
- Monitor errors do not terminate the application or recording pipeline.
- Automatic start failure transitions to suppression for the current meeting
  and sends a system error notification.
- Automatic stop failure keeps detector ownership visible in status, reports the
  error, and does not claim that the meeting was saved.
- All start and stop operations are serialized by the coordinator to prevent
  duplicate lifecycle calls across polling cycles.

## Privacy and Logging

- Detection stays local.
- The first provider reads process names only.
- No process list, meeting metadata, or detection evidence is sent to analytics.
- Info logs contain provider IDs and state transitions, not meeting titles or
  user data.
- Recording remains opt-in and continues to display Meetily's existing reminder
  to inform participants.

## Testing

Rust unit tests use fake providers and a controllable coordinator clock. They
cover:

- provider descriptor and platform support;
- Zoom positive and negative process snapshots;
- positive confirmation debounce;
- stop grace period and cancellation when the signal returns;
- automatic start and owned automatic stop;
- manual recording present before detection;
- manual stop suppression during an active meeting;
- failed start suppression;
- disabling detection during an owned recording;
- settings defaults and backward-compatible deserialization.

Frontend coverage verifies service serialization and settings control behavior
where the repository's existing test setup supports it. Verification also
includes `cargo fmt`, focused Rust tests, `cargo check`, frontend lint/tests, and
a manual Tauri smoke test on macOS with Zoom. The settings UI is inspected in a
running development build and captured at desktop and narrow widths.

## Rollout

The feature ships disabled by default. The first release labels Zoom detection
as supported on macOS. Other platforms are omitted or shown as unsupported based
on backend descriptors. Future providers implement the same interface and add
their descriptor; the monitor, coordinator, persistence format, and settings
layout remain unchanged.
