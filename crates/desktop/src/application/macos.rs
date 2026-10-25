//! Route Cmd+Q and Dock Quit through cooperative shutdown.
//!
//! Recheck our hook on the current winit delegate before each worker starts;
//! missing or foreign termination policies fail closed.
use std::{mem, ptr};

use objc2::{
    MainThreadMarker, ffi,
    runtime::{AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};

use super::with_app;

pub(super) fn install() -> bool {
    let Some(main_thread) = MainThreadMarker::new() else {
        return false;
    };
    let application = NSApplication::sharedApplication(main_thread);
    let Some(delegate) = application.delegate() else {
        return false;
    };
    let object: &AnyObject = (*delegate).as_ref();
    let class = object.class();
    if class.name() != c"WinitApplicationDelegate" {
        return false;
    }
    let selector = sel!(applicationShouldTerminate:);
    let implementation = implementation();
    // Never replace an existing termination policy.
    if let Some(method) = class.instance_method(selector) {
        return ptr::fn_addr_eq(method.implementation(), implementation);
    }
    #[expect(
        unsafe_code,
        reason = "AppKit has no safe API for extending the existing winit delegate"
    )]
    // SAFETY: AppKit invokes this selector on the main thread. The signature and Q@:@ encoding
    // match applicationShouldTerminate: (NSUInteger, self, _cmd, NSApplication*) on 64-bit macOS.
    // The static function outlives the registered class; no delegate or other method is replaced.
    let added = unsafe {
        ffi::class_addMethod(
            ptr::from_ref(class).cast_mut(),
            selector,
            implementation,
            c"Q@:@".as_ptr(),
        )
        .as_bool()
    };
    added
        && class
            .instance_method(selector)
            .is_some_and(|method| ptr::fn_addr_eq(method.implementation(), implementation))
}

#[expect(
    unsafe_code,
    reason = "Objective-C IMP erases the signature of the statically typed AppKit callback"
)]
fn implementation() -> Imp {
    // SAFETY: The callback is registered only for applicationShouldTerminate: with its
    // exact Q@:@ signature, and the function remains alive for the process lifetime.
    unsafe {
        mem::transmute::<
            extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> NSApplicationTerminateReply,
            Imp,
        >(should_terminate)
    }
}

pub(super) extern "C-unwind" fn should_terminate(
    _: &AnyObject,
    _: Sel,
    _: &AnyObject,
) -> NSApplicationTerminateReply {
    with_app(|application| application.stop(true));
    // Defer process exit until Event::Stopped ends the Slint loop.
    NSApplicationTerminateReply::TerminateCancel
}
