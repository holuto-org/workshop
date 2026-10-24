//! Strip machine-local paths and source contents without losing symbolication identifiers.

use sentry::protocol::{DebugImage, Event, Stacktrace};

pub(super) fn scrub_paths(event: &mut Event<'static>) {
    if !event.debug_meta.images.is_empty() {
        for image in &mut event.debug_meta.to_mut().images {
            match image {
                DebugImage::Apple(image) => basename(&mut image.name),
                DebugImage::Symbolic(image) => {
                    basename(&mut image.name);
                    optional_basename(&mut image.debug_file);
                },
                DebugImage::Wasm(image) => {
                    basename(&mut image.name);
                    basename(&mut image.code_file);
                    optional_basename(&mut image.debug_file);
                },
                DebugImage::Proguard(_) => {},
            }
        }
    }
    for exception in &mut event.exception.values {
        optional_basename(&mut exception.module);
        for stack in [
            exception.stacktrace.as_mut(),
            exception.raw_stacktrace.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            scrub_stack(stack);
        }
    }
    for thread in &mut event.threads.values {
        for stack in [thread.stacktrace.as_mut(), thread.raw_stacktrace.as_mut()]
            .into_iter()
            .flatten()
        {
            scrub_stack(stack);
        }
    }
    if let Some(stack) = &mut event.stacktrace {
        scrub_stack(stack);
    }
    if let Some(template) = &mut event.template {
        optional_basename(&mut template.filename);
        template.abs_path = None;
        template.pre_context.clear();
        template.context_line = None;
        template.post_context.clear();
    }
}

fn scrub_stack(stack: &mut Stacktrace) {
    for frame in &mut stack.frames {
        optional_basename(&mut frame.filename);
        optional_basename(&mut frame.module);
        optional_basename(&mut frame.package);
        frame.abs_path = None;
        frame.vars.clear();
        frame.pre_context.clear();
        frame.context_line = None;
        frame.post_context.clear();
    }
}

fn optional_basename(path: &mut Option<String>) {
    if let Some(path) = path {
        basename(path);
    }
}

fn basename(path: &mut String) {
    // SDK events may contain Windows paths even when processed on Unix, and vice versa.
    if let Some(separator) = path.rfind(['/', '\\']) {
        drop(path.drain(..=separator));
    }
}

#[cfg(test)]
#[path = "privacy/tests.rs"]
mod tests;
