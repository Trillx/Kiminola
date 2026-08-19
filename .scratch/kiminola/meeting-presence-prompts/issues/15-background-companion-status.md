# Background companion status visibility

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: Background companion startup and controls

## Question

How should the user know whether the Background companion is active, paused, or fully quit? Decide whether the tray icon/status menu is the source of truth, whether a persistent in-app setting mirrors it, and how to communicate that active means local hint detection only—not audio capture.

## Resolution

The tray icon and menu are the source of truth and Settings mirrors the same state. Use clear states: **Detecting locally · not recording**, **Paused**, and **Off**. The active state must communicate that only local presence hints are being observed and no audio is being captured.
