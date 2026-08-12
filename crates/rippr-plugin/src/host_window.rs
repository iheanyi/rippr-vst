use std::{ffi::c_ulong, num::NonZeroIsize, ptr::NonNull};

#[cfg(target_os = "macos")]
use std::ffi::c_void;

use raw_window_handle::{HasWindowHandle, RawWindowHandle as Rwh, WindowHandle};
use truce::core::editor::RawWindowHandle;

#[derive(Clone, Copy, Debug)]
pub struct HostWindow(pub RawWindowHandle);

impl HostWindow {
    #[cfg(target_os = "macos")]
    pub fn appkit_view(self) -> Option<NonNull<c_void>> {
        match self.0 {
            RawWindowHandle::AppKit(view) => NonNull::new(view),
            _ => None,
        }
    }
}

impl HasWindowHandle for HostWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = match self.0 {
            RawWindowHandle::AppKit(view) => {
                let view = NonNull::new(view).ok_or(raw_window_handle::HandleError::Unavailable)?;
                Rwh::AppKit(raw_window_handle::AppKitWindowHandle::new(view))
            }
            RawWindowHandle::Win32(window) => {
                let window = NonZeroIsize::new(window as isize)
                    .ok_or(raw_window_handle::HandleError::Unavailable)?;
                Rwh::Win32(raw_window_handle::Win32WindowHandle::new(window))
            }
            RawWindowHandle::X11(window) => {
                Rwh::Xlib(raw_window_handle::XlibWindowHandle::new(window as c_ulong))
            }
            RawWindowHandle::UiKit(_) => {
                return Err(raw_window_handle::HandleError::NotSupported);
            }
        };

        // SAFETY: Truce supplies a host-owned parent that remains valid for the editor lifetime.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}
