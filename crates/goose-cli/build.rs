// Tell cargo that GOOSE_BUILD_VERSION / GOOSE_BUILD_SHA participate in the build.
//
// `option_env!` is resolved at COMPILE time, and cargo does not know an env var it has never been told
// about has changed — so a second build with the var set reuses the first build's object files and the
// stamp silently does not land. MEASURED: I shipped the stamp, ran `GOOSE_BUILD_SHA=... cargo build`, and
// `strings` found neither the sha nor the version in the binary. Without these lines the engine would go
// on reporting "dev" forever while the build script cheerfully exported the right values.
//
// These are what `levers_resolved.version` / `.build_sha` report, i.e. how a run says which binary
// produced it.
fn main() {
    println!("cargo:rerun-if-env-changed=GOOSE_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=GOOSE_BUILD_SHA");

    // DERIVE THE SHA WHEN NOBODY EXPORTED ONE, so a plain `cargo build --release` is attributable.
    //
    // Only `just release-fork` exported GOOSE_BUILD_SHA. Every other build — including the one the
    // measurement loop's boundary runs — left it unset, so `levers_resolved.build_sha` has read the
    // literal "dev" on every run this campaign has produced. That makes a run unattributable to a
    // commit, which is precisely the question that matters after shipping a lever: DID THIS RUN
    // CONTAIN MY CHANGE? A campaign that cannot answer that from the run's own log is guessing.
    //
    // An explicit export still wins, and a build outside a git checkout falls back to "dev" exactly as
    // before. `.git/HEAD` is watched so committing or switching branches re-stamps instead of reusing a
    // stale object file — the same trap the block above documents, one level down.
    if std::env::var_os("GOOSE_BUILD_SHA").is_none() {
        let sha = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(sha) = sha {
            // A DIRTY TREE IS NOT THE COMMIT IT NAMES. Saying so in the stamp is the difference between
            // "this run contained my change" and "this run contained something like it".
            let dirty = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .output()
                .ok()
                .is_some_and(|o| !o.stdout.is_empty());
            println!(
                "cargo:rustc-env=GOOSE_BUILD_SHA={sha}{}",
                if dirty { "-dirty" } else { "" }
            );
        }
        // WATCHING `.git/HEAD` ALONE DOES NOT SEE A COMMIT, and the line below used to be the whole
        // mechanism while the comment above claimed "committing or switching branches re-stamps".
        // Only the SWITCH half was true: `git commit` on the same branch leaves `.git/HEAD` byte-for-byte
        // identical (`ref: refs/heads/<branch>`) and moves `.git/refs/heads/<branch>` instead. So cargo
        // saw no input change, reused the object file, and the stamp froze.
        //
        // MEASURED: a release binary built 2026-08-05 17:25, carrying six engine changes committed that
        // afternoon, reported `build_sha: eb8027139-dirty` — a commit from 2026-08-03 18:40, TWO DAYS
        // STALE. The field whose entire purpose is to answer "did this run contain my change?" was
        // answering about a different day, which is worse than the "dev" it replaced: "dev" is obviously
        // useless, a plausible wrong sha is believed.
        println!("cargo:rerun-if-changed=../../.git/HEAD");
        // The ref HEAD points at — this is the file a commit actually moves.
        if let Ok(head) = std::fs::read_to_string("../../.git/HEAD") {
            if let Some(r) = head.strip_prefix("ref: ").map(str::trim) {
                println!("cargo:rustc-env=GOOSE_BUILD_REF={r}");
                println!("cargo:rerun-if-changed=../../.git/{r}");
            }
        }
        // And the packed form: `git gc` moves refs out of `refs/heads/` into this single file, after
        // which the path above stops existing and watching only it would silently freeze again.
        println!("cargo:rerun-if-changed=../../.git/packed-refs");
    }
}
