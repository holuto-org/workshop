use std::{
    error::Error,
    sync::{Arc, Mutex, PoisonError},
};

use sentry::{
    Client, Envelope, Hub, Transport,
    protocol::{EnvelopeItem, Event},
};
use serde_json::{Value, json};

use super::super::options;

#[derive(Default)]
struct Capture(Mutex<Vec<Envelope>>);

impl Transport for Capture {
    fn send_envelope(&self, envelope: Envelope) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(envelope);
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn outgoing_envelopes_keep_symbols_without_local_paths() -> Result<(), Box<dyn Error>> {
    let event: Event<'static> = serde_json::from_value(json!({
        "exception": {"values": [{
            "type": "PrinterFailure",
            "module": "/home/canary-user-9183/workshop/worker",
            "stacktrace": stack("exception"),
            "raw_stacktrace": stack("raw-exception")
        }]},
        "threads": {"values": [{
            "id": 42,
            "stacktrace": stack("thread"),
            "raw_stacktrace": stack("raw-thread")
        }]},
        "stacktrace": stack("top-level"),
        "template": {
            "filename": "C:\\Users\\canary-user-9183\\view.slint",
            "abs_path": "/home/canary-user-9183/view.slint",
            "lineno": 7,
            "pre_context": ["/home/canary-user-9183/before"],
            "context_line": "C:\\Users\\canary-user-9183\\current",
            "post_context": ["/home/canary-user-9183/after"]
        },
        "debug_meta": {"images": debug_images()}
    }))?;
    let capture = Arc::new(Capture::default());
    let client = Arc::new(Client::from(
        options()
            .dsn("https://public@example.invalid/1")
            .transport(Arc::clone(&capture)),
    ));

    Hub::run(Arc::new(Hub::new(Some(client), Arc::default())), || {
        let _event_id = sentry::capture_event(event);
    });

    let envelopes = capture.0.lock().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    for envelope in envelopes.iter() {
        envelope.to_writer(&mut bytes)?;
    }
    let payload = String::from_utf8(bytes)?;
    assert!(
        !payload.contains("canary-user-9183"),
        "the complete outgoing envelope must not disclose the local username"
    );
    let events: Vec<_> = envelopes
        .iter()
        .flat_map(Envelope::items)
        .filter_map(|item| {
            if let EnvelopeItem::Event(event) = item {
                Some(event)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(events.len(), 1, "privacy must not discard the issue");
    let event = serde_json::to_value(events.first().ok_or("event missing")?)?;
    for location in [
        "/exception/values/0/stacktrace",
        "/exception/values/0/raw_stacktrace",
        "/threads/values/0/stacktrace",
        "/threads/values/0/raw_stacktrace",
        "/stacktrace",
    ] {
        assert_eq!(
            event.pointer(location),
            Some(&expected_stack()),
            "symbols, addresses and basenames survive at {location}"
        );
    }
    assert_eq!(
        event.pointer("/exception/values/0/module"),
        Some(&json!("worker")),
        "exception modules retain only their basename"
    );
    assert_eq!(
        event.get("template"),
        Some(&json!({"filename": "view.slint", "lineno": 7})),
        "template source contents and absolute paths are absent"
    );
    assert_eq!(
        event.pointer("/debug_meta/images"),
        Some(&expected_images()),
        "debug identifiers and image basenames remain available for symbolication"
    );
    Ok(())
}

fn stack(location: &str) -> Value {
    json!({
        "frames": [{
            "function": "workshop::worker::print",
            "symbol": "_ZN8workshop6worker5printE",
            "module": format!("/home/canary-user-9183/{location}/worker"),
            "package": format!("C:\\Users\\canary-user-9183\\{location}\\holuto.dll"),
            "filename": format!("C:\\Users\\canary-user-9183\\{location}\\worker.rs"),
            "abs_path": format!("/home/canary-user-9183/{location}/worker.rs"),
            "lineno": 31,
            "colno": 9,
            "in_app": true,
            "pre_context": [format!("/home/canary-user-9183/{location}/before")],
            "context_line": format!("C:\\Users\\canary-user-9183\\{location}\\current"),
            "post_context": [format!("/home/canary-user-9183/{location}/after")],
            "vars": {"settings": format!("/home/canary-user-9183/{location}/settings.json")},
            "image_addr": "0x1000",
            "instruction_addr": "0x1234",
            "symbol_addr": "0x1200",
            "addr_mode": "abs"
        }, {
            "function": "workshop::main",
            "module": "workshop::desktop",
            "package": "workshop",
            "filename": format!("/home/canary-user-9183/{location}/main.rs"),
            "abs_path": format!("C:\\Users\\canary-user-9183\\{location}\\main.rs")
        }],
        "frames_omitted": [2, 4],
        "registers": {"rip": "0x1234"}
    })
}

fn expected_stack() -> Value {
    json!({
        "frames": [{
            "function": "workshop::worker::print",
            "symbol": "_ZN8workshop6worker5printE",
            "module": "worker",
            "package": "holuto.dll",
            "filename": "worker.rs",
            "lineno": 31,
            "colno": 9,
            "in_app": true,
            "image_addr": "0x1000",
            "instruction_addr": "0x1234",
            "symbol_addr": "0x1200",
            "addr_mode": "abs"
        }, {
            "function": "workshop::main",
            "module": "workshop::desktop",
            "package": "workshop",
            "filename": "main.rs"
        }],
        "frames_omitted": [2, 4],
        "registers": {"rip": "0x1234"}
    })
}

fn debug_images() -> Value {
    json!([{
        "type": "apple",
        "name": "/Users/canary-user-9183/libholuto.dylib",
        "arch": "arm64",
        "cpu_type": null,
        "cpu_subtype": null,
        "image_addr": "0x1000",
        "image_size": 4096,
        "image_vmaddr": "0x2000",
        "uuid": "67e9247c-814e-392b-a027-dbde6748fcbf"
    }, {
        "type": "symbolic",
        "name": "C:\\Users\\canary-user-9183\\holuto.exe",
        "arch": "x86_64",
        "image_addr": "0x3000",
        "image_size": 8192,
        "image_vmaddr": "0x4000",
        "id": "67e9247c-814e-392b-a027-dbde6748fcbf",
        "code_id": "abcdef1234",
        "debug_file": "/home/canary-user-9183/holuto.pdb"
    }, {
        "type": "wasm",
        "name": "/home/canary-user-9183/holuto.wasm",
        "debug_id": "67e9247c-814e-392b-a027-dbde6748fcbf",
        "debug_file": "C:\\Users\\canary-user-9183\\holuto.wasm.debug",
        "code_id": "abc123",
        "code_file": "C:\\Users\\canary-user-9183\\holuto.wasm"
    }, {
        "type": "proguard",
        "uuid": "67e9247c-814e-392b-a027-dbde6748fcbf"
    }])
}

fn expected_images() -> Value {
    json!([{
        "type": "apple",
        "name": "libholuto.dylib",
        "arch": "arm64",
        "cpu_type": null,
        "cpu_subtype": null,
        "image_addr": "0x1000",
        "image_size": 4096,
        "image_vmaddr": "0x2000",
        "uuid": "67e9247c-814e-392b-a027-dbde6748fcbf"
    }, {
        "type": "symbolic",
        "name": "holuto.exe",
        "arch": "x86_64",
        "image_addr": "0x3000",
        "image_size": 8192,
        "image_vmaddr": "0x4000",
        "id": "67e9247c-814e-392b-a027-dbde6748fcbf",
        "code_id": "abcdef1234",
        "debug_file": "holuto.pdb"
    }, {
        "type": "wasm",
        "name": "holuto.wasm",
        "debug_id": "67e9247c-814e-392b-a027-dbde6748fcbf",
        "debug_file": "holuto.wasm.debug",
        "code_id": "abc123",
        "code_file": "holuto.wasm"
    }, {
        "type": "proguard",
        "uuid": "67e9247c-814e-392b-a027-dbde6748fcbf"
    }])
}
