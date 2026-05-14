---
id: TASK-2
title: Segment-by-segment text injection while whisper streams
status: To Do
assignee: []
created_date: '2026-05-14 16:45'
labels: []
dependencies: []
priority: medium
ordinal: 2000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Currently the full transcription text is injected only after the whisper 'done' message. faster-whisper streams segments as they are ready. Injecting each segment immediately as it arrives reduces perceived latency significantly for longer dictations.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Each whisper segment is injected at cursor as soon as it arrives
- [ ] #2 A separator (space) is inserted between segments
- [ ] #3 Overlay remains visible until all segments are injected
<!-- AC:END -->
