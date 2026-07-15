# Fork README Design

## Summary

Update the existing English README without replacing the upstream project
documentation. The revised introduction will identify this repository as a
personal fork of [Zackriya-Solutions/meetily](https://github.com/Zackriya-Solutions/meetily),
explain the local Zoom meeting automation added by the fork, and provide a
tested macOS build-and-install path for this branch.

## Goals

- Give clear attribution and a direct link to the original repository.
- Explain why the fork exists and what behavior differs from upstream.
- State the current support boundary: Zoom meeting detection on macOS.
- Document privacy, consent, and manual-recording safety behavior.
- Provide copyable commands to clone, build, and install the fork locally.
- Preserve the upstream README sections that remain applicable.

## README Structure

Add an `About This Fork` section immediately after the introductory header and
before upstream promotional content. This makes the repository relationship and
the fork-specific behavior visible without removing upstream authorship,
screenshots, product links, or general feature documentation.

The section will cover:

- the upstream repository link;
- the fork's purpose;
- local, opt-in Zoom detection with automatic recording start and stop;
- macOS-only support in the current implementation;
- ownership rules that protect manual recordings;
- the `Settings -> Automation` opt-in path.

The existing `Automatic Meeting Detection` section remains the detailed feature
description. It will be edited only where necessary to avoid duplication or to
clarify the fork boundary.

## Build and Installation Instructions

Add a `Build and Install This Fork on macOS` subsection under `Installation`.
It will target the published `enhance/meeting-auto-detection` branch and include:

1. prerequisites: Xcode Command Line Tools, Homebrew, CMake, Node.js, pnpm, and
   Rust;
2. cloning the personal fork and checking out the feature branch;
3. installing frontend dependencies;
4. running `frontend/build-gpu.sh` with a local Tauri configuration override
   that disables updater artifacts, because the upstream updater public key is
   present but its private release key is intentionally unavailable;
5. the generated `.app` and `.dmg` locations;
6. installing through the generated DMG or copying the app bundle to
   `/Applications`;
7. launching Meetily, granting macOS audio permissions, and enabling all three
   automation switches.

The instructions will distinguish local ad-hoc application signing from the
upstream project's notarized release process. They will not instruct users to
disable Gatekeeper or remove quarantine metadata.

## Verification

Before publishing:

- review the README diff for working relative links and copyable commands;
- check that every documented path matches the current build output;
- run a Markdown whitespace/error scan;
- verify that only the README and this design document are newly changed;
- commit the README update separately from the already completed feature
  commits, then push the current branch to the personal `fork` remote.

No pull request to the upstream repository will be opened unless explicitly
requested.
