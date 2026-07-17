// Tell cargo that GOOSE_BUILD_VERSION / GOOSE_BUILD_SHA participate in the build.
//
// `option_env!` is resolved at COMPILE time, and cargo does not know an env var it has never been told
// about has changed — so a second build with the var set reuses the first build's object files and the
// stamp silently does not land. MEASURED: I shipped the stamp, ran `GOOSE_BUILD_SHA=... cargo build`, and
// `strings` found neither the sha nor the version in the binary. Without these lines the engine would go
// on reporting "dev" forever while the build script cheerfully exported the right values.
//
// These are what `levers_resolved.version` / `.build_sha` report, i.e. how a run says which binary
// produced it. `just release-fork` exports them; a plain `cargo build` leaves them unset and the engine
// honestly reports "dev".
fn main() {
    println!("cargo:rerun-if-env-changed=GOOSE_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=GOOSE_BUILD_SHA");
}
