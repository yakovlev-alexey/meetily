# Meeting Auto-Detection Implementation Plan

## Objective

Implement the approved local meeting auto-detection design on the
`enhance/meeting-auto-detection` branch. Ship Zoom detection on macOS while
keeping application detection extensible through backend-provided provider
descriptors and a provider interface.

## Task 1: Detection Domain and Zoom Provider

Files:

- `frontend/src-tauri/src/meeting_detection/context.rs`
- `frontend/src-tauri/src/meeting_detection/provider.rs`
- `frontend/src-tauri/src/meeting_detection/providers/mod.rs`
- `frontend/src-tauri/src/meeting_detection/providers/zoom.rs`
- `frontend/src-tauri/src/meeting_detection/mod.rs`

Steps:

1. Add normalized process snapshot types to `DetectionContext`.
2. Define provider ID, descriptor, platform support, evidence, observation, and
   the `MeetingProvider` trait.
3. Implement `ZoomProvider` using the macOS `CptHost` active-call helper.
4. Keep the provider pure and add focused positive/negative/platform tests.

Verification:

```bash
cd frontend/src-tauri
cargo test meeting_detection::providers::zoom
```

## Task 2: Coordinator State Machine

Files:

- `frontend/src-tauri/src/meeting_detection/coordinator.rs`

Steps:

1. Model disabled, idle, confirming, starting, recording, grace-period,
   stopping, and suppressed states.
2. Feed the coordinator observations, recording activity, and monotonic time.
3. Return explicit start/stop actions instead of invoking Tauri from domain
   logic.
4. Preserve detector ownership separately from the current observation.
5. Add deterministic tests for debounce, grace cancellation, manual recording,
   manual stop, failed start, disabling detection, and stop failure.

Verification:

```bash
cd frontend/src-tauri
cargo test meeting_detection::coordinator
```

## Task 3: Settings and Provider Registry

Files:

- `frontend/src-tauri/src/meeting_detection/settings.rs`
- `frontend/src-tauri/src/meeting_detection/provider.rs`
- `frontend/src-tauri/src/meeting_detection/mod.rs`

Steps:

1. Add serde-defaulted `MeetingDetectionSettings` with the master switch,
   auto-start, auto-stop, and provider toggles.
2. Persist settings in the existing Tauri plugin store.
3. Add a provider registry that returns descriptors and detects the first active
   enabled provider from one shared context.
4. Test defaults, partial legacy JSON, and provider enablement.

Verification:

```bash
cd frontend/src-tauri
cargo test meeting_detection::settings
```

## Task 4: Monitor and Recording Lifecycle Integration

Files:

- `frontend/src-tauri/src/meeting_detection/monitor.rs`
- `frontend/src-tauri/src/meeting_detection/commands.rs`
- `frontend/src-tauri/src/lib.rs`
- `frontend/src-tauri/src/tray.rs` only if lifecycle reuse requires it

Steps:

1. Add managed monitor state with exactly one background task.
2. Refresh processes once every two seconds and apply the current settings.
3. Execute coordinator actions serially.
4. Start with configured/default audio devices and a generated Zoom meeting
   name.
5. Stop through the same graceful backend path used by tray recording and emit
   `recording-stop-complete` only after success.
6. Route lifecycle failures through existing system error notifications.
7. Register state, startup initialization, commands, and events in Tauri.
8. Add monitor-level tests using fake providers/actions where practical; keep
   OS process and Tauri runtime boundaries thin.

Verification:

```bash
cd frontend/src-tauri
cargo test meeting_detection
cargo check
```

## Task 5: Frontend Automation Settings

Files:

- `frontend/src/services/meetingDetectionService.ts`
- `frontend/src/components/MeetingDetectionSettings.tsx`
- `frontend/src/app/settings/page.tsx`

Steps:

1. Add concrete TypeScript settings/status/provider types and a focused Tauri
   service wrapper.
2. Build one settings widget that owns load/save/error state and renders simple
   presentational rows.
3. Add the `Automation` tab between `Recordings` and `Transcription`.
4. Render providers from backend descriptors, disable unsupported providers,
   and preserve settings on command failure.
5. Use the current Tailwind/shadcn patterns and existing toast behavior.

Verification:

```bash
cd frontend
pnpm lint
pnpm build
```

## Task 6: Documentation and Full Verification

Files:

- `README.md`
- relevant changed files from previous tasks

Steps:

1. Document the opt-in macOS Zoom behavior and recording-consent reminder.
2. Run formatters before checking the diff.
3. Run focused tests, the complete Rust library test set that is viable in the
   local environment, `cargo check`, frontend lint/tests, and frontend build.
4. Start the app or frontend on a non-default port, inspect the Automation tab
   at desktop and narrow widths, and save screenshots outside the repository.
5. Inspect `git diff --check`, working-tree scope, and requirement coverage.
6. Commit only feature-related files with a terse imperative message.

Verification:

```bash
cd frontend/src-tauri
cargo fmt --check
cargo test meeting_detection
cargo check

cd ../../frontend
pnpm lint
pnpm build

git diff --check
```
