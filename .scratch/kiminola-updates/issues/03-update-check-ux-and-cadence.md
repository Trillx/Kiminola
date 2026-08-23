Type: grilling
Status: resolved

## Question

What exact user experience completes the agreed stable update policy without interrupting recording?

## Answer

The settled implementation is one automatic, non-blocking check about two seconds after the main app shell launches, plus a manual Settings check. An available stable release appears in a global banner and in Settings with compact release notes, download progress, and explicit `Install update` / `Restart and update` actions. `Later` dismisses that version for the current UI session.

Installation is disabled while the current route is `/record` and the guard is checked again after download, so navigation or a newly started recording cannot be interrupted by the final install call. The passive Windows updater closes, replaces, and restarts the app after installation. Errors remain visible in Settings and never block the recording flow.
