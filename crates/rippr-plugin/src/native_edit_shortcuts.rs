#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditAction {
    Copy,
    Cut,
    Paste,
    SelectAll,
    Undo,
    Redo,
}

#[cfg(any(target_os = "macos", test))]
fn edit_action(
    key: &str,
    command: bool,
    shift: bool,
    control: bool,
    option: bool,
) -> Option<EditAction> {
    if !command || control || option {
        return None;
    }

    match (key.to_ascii_lowercase().as_str(), shift) {
        ("c", false) => Some(EditAction::Copy),
        ("x", false) => Some(EditAction::Cut),
        ("v", false) => Some(EditAction::Paste),
        ("a", false) => Some(EditAction::SelectAll),
        ("z", false) => Some(EditAction::Undo),
        ("z", true) | ("y", false) => Some(EditAction::Redo),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSView};

    use crate::host_window::HostWindow;

    use super::{EditAction, edit_action};

    pub struct NativeEditShortcuts {
        monitor: Option<Retained<AnyObject>>,
    }

    impl NativeEditShortcuts {
        pub fn new(parent: HostWindow) -> Self {
            let Some(parent_view) = parent.appkit_view() else {
                return Self { monitor: None };
            };
            if MainThreadMarker::new().is_none() {
                return Self { monitor: None };
            }

            let block = RcBlock::new(move |event_ptr: NonNull<NSEvent>| {
                // SAFETY: AppKit provides a live NSEvent for the duration of the monitor callback.
                let event = unsafe { event_ptr.as_ref() };
                let Some(mtm) = MainThreadMarker::new() else {
                    return event_ptr.as_ptr();
                };
                // SAFETY: The host owns this NSView for the editor lifetime, and the monitor is
                // removed before the editor resources (and therefore the parent view) are dropped.
                let view = unsafe { parent_view.cast::<NSView>().as_ref() };
                let Some(window) = view.window() else {
                    return event_ptr.as_ptr();
                };
                if window.windowNumber() != event.windowNumber() {
                    return event_ptr.as_ptr();
                }

                let flags = event.modifierFlags();
                let Some(key) = event.charactersIgnoringModifiers() else {
                    return event_ptr.as_ptr();
                };
                let Some(action) = edit_action(
                    &key.to_string(),
                    flags.contains(NSEventModifierFlags::Command),
                    flags.contains(NSEventModifierFlags::Shift),
                    flags.contains(NSEventModifierFlags::Control),
                    flags.contains(NSEventModifierFlags::Option),
                ) else {
                    return event_ptr.as_ptr();
                };

                let selector = match action {
                    EditAction::Copy => sel!(copy:),
                    EditAction::Cut => sel!(cut:),
                    EditAction::Paste => sel!(paste:),
                    EditAction::SelectAll => sel!(selectAll:),
                    EditAction::Undo => sel!(undo:),
                    EditAction::Redo => sel!(redo:),
                };
                let application = NSApplication::sharedApplication(mtm);
                // SAFETY: These are standard NSResponder edit selectors. A nil target asks
                // AppKit to resolve the action through the active first-responder chain.
                let handled = unsafe { application.sendAction_to_from(selector, None, Some(view)) };
                if handled {
                    std::ptr::null_mut()
                } else {
                    event_ptr.as_ptr()
                }
            });

            // SAFETY: The block always returns either the event AppKit supplied or null to consume
            // it. The retained monitor is removed in Drop before the captured view can go away.
            let monitor = unsafe {
                NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
            };
            Self { monitor }
        }
    }

    impl Drop for NativeEditShortcuts {
        fn drop(&mut self) {
            if let Some(monitor) = self.monitor.take() {
                // SAFETY: `monitor` is the token returned by AppKit's local monitor API and it is
                // removed exactly once here.
                unsafe { NSEvent::removeMonitor(&monitor) };
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use crate::host_window::HostWindow;

    pub struct NativeEditShortcuts;

    impl NativeEditShortcuts {
        pub fn new(_parent: HostWindow) -> Self {
            Self
        }
    }
}

pub use platform::NativeEditShortcuts;

#[cfg(test)]
mod tests {
    use super::{EditAction, edit_action};

    #[test]
    fn maps_standard_macos_edit_shortcuts_without_stealing_modified_keys() {
        assert_eq!(
            edit_action("v", true, false, false, false),
            Some(EditAction::Paste)
        );
        assert_eq!(
            edit_action("A", true, false, false, false),
            Some(EditAction::SelectAll)
        );
        assert_eq!(
            edit_action("z", true, true, false, false),
            Some(EditAction::Redo)
        );
        assert_eq!(edit_action("v", false, false, false, false), None);
        assert_eq!(edit_action("v", true, false, false, true), None);
    }
}
