//! Trackpad pinches, read from AppKit.
//!
//! gpui 0.2.2 doesn't deliver macOS magnify events: its window ignores them. A local event
//! monitor sees every event before gpui dispatches it, so it reads each step of a pinch, passes
//! the event on unchanged, and forwards the step to the window.

use tokio::sync::mpsc::UnboundedSender;

/// One step of a pinch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pinch {
    /// How much this step zooms. Above 1 zooms in.
    pub factor: f64,
    /// The pointer's x position in the window, in pixels.
    pub x: f32,
}

/// Forwards pinches to the window while it lives.
pub struct PinchMonitor {
    #[cfg(target_os = "macos")]
    monitor: *mut objc::runtime::Object,
}

#[cfg(target_os = "macos")]
impl PinchMonitor {
    /// `NSEventMaskMagnify`: `1 << NSEventTypeMagnify`, which is 30.
    const MAGNIFY_MASK: u64 = 1 << 30;

    /// Starts forwarding pinches. Call it on the main thread, after the app launched.
    pub fn install(pinches: UnboundedSender<Pinch>) -> Self {
        use block::ConcreteBlock;
        use objc::{class, msg_send, runtime::Object, sel, sel_impl};

        /// `NSPoint`, which AppKit returns for `locationInWindow`.
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Point {
            x: f64,
            y: f64,
        }

        let handler = ConcreteBlock::new(move |event: *mut Object| -> *mut Object {
            // SAFETY: AppKit passes a valid `NSEvent` of the masked type, on the main thread.
            let (magnification, location): (f64, Point) = unsafe {
                (
                    msg_send![event, magnification],
                    msg_send![event, locationInWindow],
                )
            };
            let _ = pinches.send(Pinch {
                factor: 1.0 + magnification,
                x: location.x as f32,
            });
            event
        })
        .copy();
        // SAFETY: AppKit copies the block, and returns a monitor that stays valid until
        // `removeMonitor:`.
        let monitor: *mut Object = unsafe {
            msg_send![
                class!(NSEvent),
                addLocalMonitorForEventsMatchingMask: Self::MAGNIFY_MASK
                handler: &*handler
            ]
        };
        // SAFETY: the monitor is an Objective-C object, which this keeps until it is dropped.
        let monitor: *mut Object = unsafe { msg_send![monitor, retain] };
        Self { monitor }
    }
}

#[cfg(target_os = "macos")]
impl Drop for PinchMonitor {
    fn drop(&mut self) {
        use objc::{class, msg_send, sel, sel_impl};
        // SAFETY: the monitor came from `addLocalMonitorForEventsMatchingMask:handler:`.
        unsafe {
            let _: () = msg_send![class!(NSEvent), removeMonitor: self.monitor];
            let _: () = msg_send![self.monitor, release];
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl PinchMonitor {
    /// Other platforms don't forward pinches yet.
    pub fn install(_pinches: UnboundedSender<Pinch>) -> Self {
        Self {}
    }
}
