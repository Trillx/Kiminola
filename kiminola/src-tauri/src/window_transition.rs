//! Pure geometry and lifetime rules shared by the native transition and tests.
use std::time::Duration;

pub(super) const DURATION: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Bounds {
    pub fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }
    pub fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }

    pub fn interpolate(self, target: Self, progress: f64) -> Self {
        let lerp = |a: i32, b: i32| {
            (f64::from(a) + (f64::from(b) - f64::from(a)) * progress).round() as i32
        };
        Self {
            left: lerp(self.left, target.left),
            top: lerp(self.top, target.top),
            right: lerp(self.right, target.right),
            bottom: lerp(self.bottom, target.bottom),
        }
    }
}

pub(super) fn split_work_area(
    area: Bounds,
    minimum_notes_width: i32,
    dpi: u32,
) -> (Bounds, Bounds) {
    let meeting_min = (480.0 * f64::from(dpi) / 96.0).ceil() as i32;
    let notes_width = (area.width() / 3)
        .max(minimum_notes_width)
        .min(area.width().saturating_sub(meeting_min).max(1));
    let edge = area.right.saturating_sub(notes_width);
    (
        Bounds {
            right: edge,
            ..area
        },
        Bounds { left: edge, ..area },
    )
}

/// Cancellation wins even over a final frame or a reduced-motion placement.
pub(super) fn progress(
    elapsed: Duration,
    animate: bool,
    cancelled: bool,
    current: bool,
) -> Option<f64> {
    if cancelled || !current {
        return None;
    }
    if !animate {
        return Some(1.0);
    }
    let t = (elapsed.as_secs_f64() / DURATION.as_secs_f64()).clamp(0.0, 1.0);
    Some(1.0 - (1.0 - t).powi(3))
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: Bounds = Bounds {
        left: -1900,
        top: -100,
        right: -700,
        bottom: 700,
    };
    const END: Bounds = Bounds {
        left: -640,
        top: 0,
        right: 0,
        bottom: 1040,
    };

    #[test]
    fn endpoints_and_delayed_final_frame_are_exact() {
        for (elapsed, expected) in [(0, START), (200, END), (850, END)] {
            let p = progress(Duration::from_millis(elapsed), true, false, true).unwrap();
            assert_eq!(START.interpolate(END, p), expected);
        }
    }

    #[test]
    fn easing_is_monotonic_and_never_overshoots() {
        let mut previous = 0.0;
        for ms in 0..=220 {
            let p = progress(Duration::from_millis(ms), true, false, true).unwrap();
            assert!((previous..=1.0).contains(&p));
            let rect = START.interpolate(END, p);
            assert!((START.left..=END.left).contains(&rect.left));
            assert!(rect.width() > 0 && rect.height() > 0);
            previous = p;
        }
        assert_eq!(
            progress(Duration::from_millis(100), true, false, true),
            Some(0.875)
        );
    }

    #[test]
    fn cancellation_or_replacement_never_writes_a_final_frame() {
        for animate in [true, false] {
            for ms in [0, 80, 200, 500] {
                assert_eq!(
                    progress(Duration::from_millis(ms), animate, true, true),
                    None
                );
                assert_eq!(
                    progress(Duration::from_millis(ms), animate, false, false),
                    None
                );
            }
        }
    }

    #[test]
    fn reduced_motion_is_immediate() {
        assert_eq!(progress(Duration::ZERO, false, false, true), Some(1.0));
    }

    #[test]
    fn layouts_use_physical_pixels_at_each_supported_scale() {
        for dpi in [96, 144, 192] {
            let scale = dpi as i32 / 48;
            let width = 1920 * scale / 2;
            let area = Bounds {
                left: -width,
                top: -80,
                right: 0,
                bottom: 1000,
            };
            let (meeting, notes) = split_work_area(area, 420 * scale / 2, dpi);
            assert_eq!(notes.width(), width / 3);
            assert_eq!(meeting.width(), width * 2 / 3);
            assert_eq!(meeting.right, notes.left);
            assert_eq!(notes.right, 0);
            assert_eq!(notes.top, -80);
        }
    }

    #[test]
    fn narrow_layout_respects_minimums_where_space_allows() {
        let area = Bounds {
            left: 0,
            top: 0,
            right: 1024,
            bottom: 768,
        };
        let (meeting, notes) = split_work_area(area, 420, 96);
        assert_eq!((meeting.width(), notes.width()), (604, 420));
    }
}
