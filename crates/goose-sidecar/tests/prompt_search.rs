//! Q-162: the tensor rank's prompt-cache search (`rank_prompt_search.py`) against mlx_lm 0.31.3's
//! own `PromptTrie.search`, kept VERBATIM in `fixtures/mlx_lm_0_31_3_prompt_trie.py`. Every rank
//! runs its own cache over the same requests, so the answer must be upstream's exactly (the same
//! entry among equally short ones, the same errors); and the time must grow with the trie, not
//! with its square — E2E #3e's rank 0 sat 22 s in upstream's search over a 259,408-token prompt
//! before goosed's hang rule stopped the split. Both run on `/usr/bin/python3`, pure stdlib.

const UPSTREAM: &str = include_str!("fixtures/mlx_lm_0_31_3_prompt_trie.py");
const SEARCH: &str = include_str!("../src/distributed/rank_prompt_search.py");

fn run_python(checks: &str) -> serde_json::Value {
    let out = std::process::Command::new("/usr/bin/python3")
        .arg("-c")
        .arg(format!("{UPSTREAM}\n{SEARCH}\n{checks}"))
        .output()
        .expect("/usr/bin/python3 runs the rank programs' pure half");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("GOOSE_TEST "))
        .unwrap_or_else(|| panic!("{stdout}{stderr}"));
    serde_json::from_str(line).unwrap()
}

/// Random tries over a four-token alphabet — many branches, many equally short entries, prefixes
/// popped as mlx_lm's insert pops them — searched both ways; every result and every error equal.
/// The tallies prove the walk below the prompt's exit (`longer`) and its ties were exercised.
#[test]
fn the_search_answers_exactly_what_mlx_lms_own_answers() {
    let seen = run_python(
        r#"
import json, random
rng = random.Random(162)
searches = longer = ties = 0
for trial in range(4000):
    trie = PromptTrie()
    entries = []
    for _ in range(rng.randint(0, 12)):
        tokens = [rng.randint(0, 3) for _ in range(rng.randint(0, 12))]
        trie.add("m", tokens, object())
        entries.append(tokens)
        if rng.random() < 0.2:
            try:
                trie.pop_prefixes("m", [rng.randint(0, 3) for _ in range(rng.randint(0, 8))])
            except KeyError:
                pass
    for _ in range(5):
        prompt = [rng.randint(0, 3) for _ in range(rng.randint(0, 14))]
        model = "m" if rng.random() < 0.95 else "other"
        try:
            upstream, upstream_error = trie.search(model, prompt), None
        except Exception as error:
            upstream, upstream_error = None, repr(error)
        try:
            ours, our_error = PromptTrieResult(*nearest_prompt(trie._trie, model, prompt)), None
        except Exception as error:
            ours, our_error = None, repr(error)
        assert (upstream, upstream_error) == (ours, our_error), (prompt, upstream, ours)
        searches += 1
        if upstream is not None and upstream.longer is not None:
            longer += 1
            depth = len(upstream.longer)
            ties += sum(1 for e in entries if len(e) == depth and e[: upstream.common_prefix]
                        == prompt[: upstream.common_prefix] and e != upstream.longer) > 0
print("GOOSE_TEST " + json.dumps({"searches": searches, "longer": longer, "ties": ties}))
"#,
    );
    assert_eq!(seen["searches"], 20_000);
    assert!(seen["longer"].as_u64().unwrap() > 1_000, "{seen}");
    assert!(seen["ties"].as_u64().unwrap() > 100, "{seen}");
}

/// E2E #3e's cache at 07:27:23 ("Prompt Cache: 2 sequences, 17.22 GB": a system entry and the
/// assistant entry that extends it) and a prompt that leaves their shared path after 3 tokens,
/// scaled to `n` prompt tokens. Upstream copies the path at every node it walks, so its time
/// grows with n²; ours grows with n. Measured by hand at the full 259,408 tokens on this Mac:
/// upstream 164,329 ms, ours 32 ms, identical results.
#[test]
fn the_search_grows_with_the_trie_not_its_square() {
    let seen = run_python(
        r#"
import json, random, time
rng = random.Random(3)

def evidence(n):
    system = [1, 2, 3] + [rng.randrange(150000) for _ in range(n + 190 - 3)]
    trie = PromptTrie()
    trie.add("m", system, "system")
    trie.add("m", system + [rng.randrange(150000) for _ in range(n // 144)], "assistant")
    return trie, [1, 2, 3] + [rng.randrange(150000) for _ in range(n - 3)]

def best_ms(search, trie, prompt, runs):
    times = []
    for _ in range(runs):
        started = time.perf_counter()
        result = search(trie, prompt)
        times.append((time.perf_counter() - started) * 1000)
    return min(times), result

ours = lambda trie, prompt: PromptTrieResult(*nearest_prompt(trie._trie, "m", prompt))
theirs = lambda trie, prompt: trie.search("m", prompt)
small, small_prompt = evidence(8192)
large, large_prompt = evidence(8 * 8192)
full, full_prompt = evidence(259408)
ours_small, a = best_ms(ours, small, small_prompt, 5)
theirs_small, b = best_ms(theirs, small, small_prompt, 3)
assert a == b
ours_large, _ = best_ms(ours, large, large_prompt, 5)
ours_full, found = best_ms(ours, full, full_prompt, 3)
print("GOOSE_TEST " + json.dumps({
    "ours_small_ms": ours_small, "theirs_small_ms": theirs_small, "ours_large_ms": ours_large,
    "ours_full_ms": ours_full, "full_longer": len(found.longer),
}))
"#,
    );
    let ms = |key: &str| seen[key].as_f64().unwrap();
    eprintln!("{seen}");
    assert_eq!(seen["full_longer"], 259_408 + 190);
    // 8× the tokens: a linear walk takes ~8× as long, a quadratic one ~64×.
    assert!(
        ms("ours_large_ms") < 32.0 * ms("ours_small_ms"),
        "8× the prompt took {:.1}× as long: {seen}",
        ms("ours_large_ms") / ms("ours_small_ms")
    );
    assert!(
        ms("theirs_small_ms") > 8.0 * ms("ours_small_ms"),
        "upstream is quadratic already at 8,192 tokens: {seen}"
    );
}
