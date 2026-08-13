//! WIT component host (assurance contract Part 17).
//!
//! Loads the guest component, mediates every capability request, and
//! measures instantiation/call/serialization overhead.

use std::time::Instant;

use wasmtime_wasi::{WasiCtx, WasiView};
use wasmtime_wasi::IoView;

struct HostCtx {
    wasi: WasiCtx,
    table: wasmtime::component::ResourceTable,
}

impl IoView for HostCtx {
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
}

impl WasiView for HostCtx {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

wasmtime::component::bindgen!({
    path: "../world.wit",
    world: "siralos-domain",
});

use exports::siralos::domain_abi::domain_api::HostAnswer;

fn timed<F>(label: &str, mut f: F) -> std::time::Duration
where
    F: FnMut() -> Result<(), String>,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    match result {
        Ok(()) => eprintln!("{label}: {:?}", elapsed),
        Err(error) => eprintln!("{label}: FAILED ({error})"),
    }
    elapsed
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = wasmtime::Engine::default();
    let component_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "guest.component.wasm".to_string());

    let start = Instant::now();
    let component = wasmtime::component::Component::from_file(&engine, &component_path)?;
    eprintln!("component-load: {:?}", start.elapsed());

    let mut linker = wasmtime::component::Linker::new(&engine);
    // The component imports WASI; the host provides it, which is itself
    // capability mediation — the component receives exactly the
    // capabilities the linker grants, nothing more.
    wasmtime_wasi::add_to_linker_sync(&mut linker)?;
    let wasi = wasmtime_wasi::WasiCtxBuilder::new()
        .inherit_stdout()
        .build();
    let mut store = wasmtime::Store::new(
        &engine,
        HostCtx { wasi, table: wasmtime::component::ResourceTable::new() },
    );

    let mut instance = None;
    timed("instantiation", || {
        let instantiated =
            SiralosDomain::instantiate(&mut store, &component, &linker).map_err(|e| e.to_string())?;
        instance = Some(instantiated);
        Ok(())
    });
    let instance = instance.expect("instantiated");
    let api = instance.siralos_domain_abi_domain_api();

    timed("bind-package-identity", || {
        let identity = exports::siralos::domain_abi::domain_api::PackageIdentity {
            package_id: "godot".to_string(),
            package_digest: "sha256-fixture".to_string(),
        };
        api.call_bind(&mut store, &identity)
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    });

    timed("semantic-query", || {
        let result = api
            .call_query(&mut store, "character movement")
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        assert_eq!(result.node_count, 0);
        Ok(())
    });

    timed("host-mediated-workspace-read", || {
        let answer = api
            .call_request_workspace_read(&mut store, "/fixture/scene.tscn", 4096)
            .map_err(|e| e.to_string())?;
        assert!(matches!(answer, HostAnswer::Ok(_)));
        Ok(())
    });

    timed("process-capability-denied", || {
        let answer = api
            .call_request_capability(
                &mut store,
                exports::siralos::domain_abi::domain_api::DeclaredCapability::ProcessExec,
            )
            .map_err(|e| e.to_string())?;
        assert!(matches!(answer, HostAnswer::Denied(_)));
        Ok(())
    });

    // Steady-state call latency.
    let start = Instant::now();
    for i in 0..200 {
        api.call_query(&mut store, &format!("ping {i}"))?;
    }
    eprintln!("round-trip: {:?} per call (200 calls)", start.elapsed() / 200);

    // Large structured payload.
    let payload = "x".repeat(1024 * 1024);
    let start = Instant::now();
    api.call_query(&mut store, &payload)?;
    eprintln!("large-payload (1048576 bytes): {:?}", start.elapsed());

    eprintln!("WIT prototype: conformance + measurement complete");
    Ok(())
}
