#[cfg(target_os = "macos")]
mod platform {
    use std::{cell::RefCell, ffi::c_void, path::Path, ptr::NonNull};

    use nice_plug::prelude::ParentWindowHandle;
    use objc2::{
        AnyThread, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
    };
    use objc2_app_kit::{
        NSApplication, NSDragOperation, NSDraggingItem, NSDraggingSession, NSDraggingSource,
        NSPasteboardWriting, NSView, NSWorkspace,
    };
    use objc2_foundation::{
        MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
        NSURL,
    };

    #[derive(Default)]
    struct DragSourceIvars;

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and DragSource does not implement Drop.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = DragSourceIvars]
        struct DragSource;

        // SAFETY: NSObjectProtocol has no additional safety requirements.
        unsafe impl NSObjectProtocol for DragSource {}

        // SAFETY: The declared method signature matches NSDraggingSource.
        unsafe impl NSDraggingSource for DragSource {
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn source_operation_mask(
                &self,
                _session: &NSDraggingSession,
                _context: objc2_app_kit::NSDraggingContext,
            ) -> NSDragOperation {
                NSDragOperation::Copy
            }
        }
    );

    impl DragSource {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(DragSourceIvars);
            // SAFETY: This is NSObject's standard initializer.
            unsafe { msg_send![super(this), init] }
        }
    }

    pub struct NativeDragContext {
        parent_view: Option<NonNull<c_void>>,
        source: Option<Retained<DragSource>>,
        session: RefCell<Option<Retained<NSDraggingSession>>>,
    }

    impl NativeDragContext {
        pub fn new(parent: ParentWindowHandle) -> Self {
            let parent_view = match parent {
                ParentWindowHandle::AppKitNsView(view) => Some(view),
                _ => None,
            };
            let source = MainThreadMarker::new().map(DragSource::new);
            Self {
                parent_view,
                source,
                session: RefCell::new(None),
            }
        }

        pub fn start(&self, path: &Path) -> Result<(), String> {
            let mtm = MainThreadMarker::new()
                .ok_or_else(|| "WAV dragging must start on the macOS UI thread.".to_string())?;
            let parent_view = self
                .parent_view
                .ok_or_else(|| "The host did not provide a macOS editor view.".to_string())?;
            let source = self
                .source
                .as_deref()
                .ok_or_else(|| "The native WAV drag source is unavailable.".to_string())?;
            if !path.is_file() {
                return Err("The active WAV is missing. Try preparing it again.".into());
            }

            let application = NSApplication::sharedApplication(mtm);
            let event = application.currentEvent().ok_or_else(|| {
                "Press and drag the handle in one motion to move the WAV into your DAW.".to_string()
            })?;

            let path_string = NSString::from_str(&path.to_string_lossy());
            let file_url = NSURL::fileURLWithPath(&path_string);
            let pasteboard_writer: &ProtocolObject<dyn NSPasteboardWriting> =
                ProtocolObject::from_ref(&*file_url);
            let item = NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                pasteboard_writer,
            );

            // SAFETY: The VST3 host owns the parent NSView for the editor lifetime, and the
            // context never outlives the editor command handler attached beneath that view.
            let view = unsafe { parent_view.cast::<NSView>().as_ref() };
            let location = view.convertPoint_fromView(event.locationInWindow(), None);
            let frame = NSRect::new(
                NSPoint::new(location.x - 24.0, location.y - 24.0),
                NSSize::new(48.0, 48.0),
            );
            let icon = NSWorkspace::sharedWorkspace().iconForFile(&path_string);
            // SAFETY: NSImage is the documented contents type for a dragging item image.
            unsafe { item.setDraggingFrame_contents(frame, Some(&icon)) };

            let items = NSArray::from_retained_slice(&[item]);
            let source: &ProtocolObject<dyn NSDraggingSource> = ProtocolObject::from_ref(source);
            let session = view.beginDraggingSessionWithItems_event_source(&items, &event, source);
            session.setAnimatesToStartingPositionsOnCancelOrFail(true);
            *self.session.borrow_mut() = Some(session);
            Ok(())
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    use nice_plug::prelude::ParentWindowHandle;

    pub struct NativeDragContext;

    impl NativeDragContext {
        pub fn new(_parent: ParentWindowHandle) -> Self {
            Self
        }

        pub fn start(&self, _path: &Path) -> Result<(), String> {
            Err("Native WAV drag-out is currently available on macOS only.".into())
        }
    }
}

pub use platform::NativeDragContext;
