# Window transition verification

Implemented September 6, 2026. Companion placement uses a 200 ms cubic ease-out transition on a dedicated worker. Each frame changes position and size together, with one elapsed-time sample for both windows. Temporary WinEvent hooks cancel on user move/resize, minimization, or destruction; visibility and window validity are also checked before writes. New requests supersede the current generation. There are no layout watchers after completion.

Hidden or minimized Kimi Nola uses its destination as its normal placement before revealing. Maximized windows restore before their start rectangles are measured. Fullscreen/presentation meeting windows remain untouched. Reduced motion uses immediate placement. Viewport resizing uses CSS geometry with no transition; an explicit sidebar-button click enables a 200 ms shell transition until it completes or a resize interrupts it.

## Verified automatically

- Svelte check: zero errors and warnings. Production frontend build passed.
- Existing frontend suite: 36 tests passed.
- Focused Rust tests: 12 passed for ARM64 and 12 for x64. The x64 executable ran under Windows emulation on the ARM64 host, not on a separate x64 machine.
- Native fixtures use temporary offscreen windows. They verify both final rectangles, actual WinEvent move-start delivery for either window, cancellation without a final snap, replacement requests, destruction during reduced motion, and hook cleanup.
- Pure geometry tests cover exact endpoints, delayed frames, monotonic easing, cancellation precedence, narrow layouts, negative screen coordinates, and 100%, 150%, and 200% scaling.
- Headless Edge runs the built library, note editor, and recording page at 1200, 761, 760, 759, 420, 760, 761, and 1200 CSS pixels. No horizontal overflow or trailing shell geometry transitions were detected. Note/recording editor text, focus, selection, and scroll position were preserved. Sidebar-button animation, interruption by resize, persisted preference, and reduced motion passed.

The browser test mocks native data and recording commands. It does not use the microphone, real notes, or an actual meeting.

## Reproduce

From `kiminola/`, run `npm run check`, `npm run build`, and `npm test`. With Playwright installed or provided by the environment, run `node tests/window-resize.browser.mjs`. An external Playwright installation can be supplied through `KIMINOLA_PLAYWRIGHT_MODULE`, pointing to its `index.mjs`; `KIMINOLA_BROWSER_CHANNEL` overrides the default Edge channel on Windows.

From `kiminola/src-tauri/`, set `SHERPA_ONNX_LIB_DIR` and `ORT_LIB_PATH` to the matching native package's `lib` directory, set `ORT_PREFER_DYNAMIC_LINK=1`, and prepend that directory and LLVM's `bin` directory to `PATH`. Run `cargo test --locked --offline --lib window_layout` for the host target, or add `--target x86_64-pc-windows-msvc` with the x64 libraries.

## Desktop checks still required

No before/after desktop recording was captured. The app was not running at the start of this task. Automated fixtures and browser checks do not prove compositor smoothness or live audio behavior.

- In a real Teams or Zoom call, start recording from the meeting prompt with both windows visible. Confirm a short coordinated glide, no flashes or overshoot, and uninterrupted audio/transcription.
- Repeat with Kimi Nola hidden, minimized, and maximized, and with the meeting window maximized or minimized. Hidden/minimized notes should appear at the destination; visible windows should finish at the intended work-area split.
- Check fullscreen/presentation protection and the missing-meeting-window fallback. Confirm no automatic rearrangement after the initial transition or after recording stops.
- During the glide, drag either window, close/hide either window, and issue a replacement layout request. Confirm the user's action wins without a later snap or refocus.
- Repeat with Windows animations disabled, on actual x64 hardware, and across monitors at 100%, 150%, and 200% scaling, including a display left of the primary display and taskbars on another edge.
- Drag the actual Tauri window through the compact breakpoint in the library, notes, and recording views. Check for blank frames and compositor stutter that headless DOM checks cannot observe.

Third-party windows can reject placement or impose their own minimum size. Native positioning calls are serialized on the worker; an unresponsive external window can delay that worker, although it does not block the app's recording-start command or queue animation frames.
