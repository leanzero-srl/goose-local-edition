//! The contextual pitfall library: KNOWN-CORRECT conventions the weak fleet routinely
//! misremembers, retrieved deterministically per task and injected into the author's prompt
//! (and read whole by the reviewer/skeptic/refuter). Sibling module under the incremental-split
//! law (development_gates::swarm_rs_line_count_only_decreases).

/// EXTERNAL GROUND TRUTH injected into the `domain-conventions` review (and its verify). Each line is a
/// KNOWN-CORRECT convention the weak fleet routinely misremembers, so the reviewer checks the produced code
/// AGAINST a fact the code+tests never generated — breaking the self-consistency that lets a shared domain
/// bug (e.g. cronmate matching a cron dow field against `dt.weekday()`) pass 79 tests + review + complete.
/// Empirically verified: a local 27B given this list + cronmate flagged the exact dow off-by-one.
pub(super) const DOMAIN_PITFALLS: &str = "\
1. CRON day-of-week is 0=Sunday..6=Saturday (7 also = Sunday). Python datetime.weekday() is 0=Monday..6=Sunday. \
Matching a cron dow field directly against dt.weekday() is an OFF-BY-ONE; correct is dt.isoweekday() % 7. \
CRON minute 0-59, hour 0-23, day-of-month 1-31, month 1-12 (1-indexed, not 0).
2. Timezone: a naive datetime is NOT UTC. Never mix naive and aware datetimes. Wall-clock scheduling must \
account for DST — adding timedelta(days=1) to an aware local time can shift the wall hour by +/-1h; round-trip \
through zoneinfo. Unix time is SECONDS since 1970 UTC; JS Date.now()/many APIs are MILLISECONDS (1000x error).
3. 0-indexing vs 1-indexing: day-of-month, months, ISO weeks, human 'Nth' are 1-based; list/array offsets are \
0-based. Do not index a 1-based value straight into a 0-based array.
4. Range inclusivity: range(a,b) and Python slices are END-EXCLUSIVE; cron 'a-b', SQL BETWEEN, and most human \
'from a to b' are END-INCLUSIVE. An inclusive upper bound needs range(a, b+1). Time buckets are usually [start,end).
5. Money/currency MUST NOT be a binary float (0.1+0.2 != 0.3). Use integer minor units (cents) or Decimal with \
an explicit rounding mode. round() in Python 3 is banker's rounding (round half to even): round(0.5)==0. \
THE EXPONENT IS PER CURRENCY, NOT 2: JPY has 0 decimals, KWD has 3, EUR/USD have 2 — dividing every \
amount by 100 renders JPY 100x too small and KWD 10x too large, and a UI that hardcodes two decimals is \
WRONG for half the rows (MEASURED: a build lost its whole money tier exactly this way). And NEVER SUM \
ACROSS CURRENCIES: a total over mixed EUR/USD/JPY/KWD rows is meaningless whatever the arithmetic — \
group by currency and total each separately, or the sum is a bug the tests will find.
6. Off-by-one at boundaries: <= vs <, first/last element, fencepost (N items -> N-1 gaps), inclusive date-diff \
(today..today is 1 day inclusive), pagination page N offset = (N-1)*size.
7. Leap years: Feb has 29 days when year%4==0 AND (year%100!=0 OR year%400==0); a year is not always 365 days.
8. String length: len(s) counts Unicode code points, not bytes (len(s.encode('utf-8'))) and not graphemes; \
truncating by bytes can split a multibyte sequence. Case-insensitive compare needs casefold(), not lower().
9. Integer vs true division: Python '/' is float, '//' floors toward -inf (-7//2 == -4); C/Go/Rust int '/' \
truncates toward zero; modulo sign follows the dividend in C, the divisor in Python. Choose deliberately for negatives.
10. Mutable default args (def f(x, acc=[])) are created ONCE and shared across calls; use a None sentinel. \
Default string sort is lexicographic/codepoint, not numeric ('10' < '2') and not locale/case-insensitive.
11. Week start: Sunday in the US locale, Monday in ISO-8601. Business-day math skips weekends (and often \
holidays); day+1 is not always the next business day. Percentages: 5% is 0.05, basis points are /10000.
12. An outbound HTTP call needs an EXPLICIT timeout: Python requests and urllib.urlopen default to NO \
timeout and Go's zero-value http.Client has none, so a server that accepts the connection and never \
answers hangs the caller forever with no recovery. Pass a timeout on every request (connect AND read), \
and bound any retry loop with a maximum attempt count plus backoff — an unbounded retry is the same \
hang with extra steps.
13. Conditional requests over a PAGED collection: an ETag belongs to ONE page, not to the collection. \
Store the validator per page key (path + cursor/offset + limit) and replay each page's OWN validator; \
one ETag reused for every page matches nothing and re-fetches the whole collection. Worse, a 304 means \
'THIS PAGE is unchanged — keep its stored rows and CONTINUE to the next page', never 'the collection is \
unchanged, stop': treating it as a stop condition ends the loop on page 1 and returns a partial list \
(the measured bug is a method documented as returning EVERY item returning exactly one page of them, \
and only on the SECOND call, because the first call populated the cache). A method that promises every \
item must page to the very end and return the accumulated list on EVERY call; a count method must \
report the collection's true total from the vendor's own total field, not the length of whatever is \
cached locally. NEVER re-issue an identical request because of the STATUS it returned: an unchanged \
conditional request answers 304 every time, so 'got 304, try again' is an infinite loop, not a retry. \
This is measured, not hypothetical — one build stored a single collection-wide validator, the vendor \
expired a cursor mid-run, the client restarted pagination while still replaying that validator, its \
own first page answered 304, and the loop re-sent that request 249,703 times: the sync never \
returned and the server went dark behind it. When pagination restarts, DROP the validators for the \
pages you are about to re-fetch.
14. sqlite3 connections are SINGLE-THREAD by default: a connection created on the main thread and \
touched from a server/worker thread raises sqlite3.ProgrammingError AT REQUEST TIME and, if the \
server runs in a thread, kills the server before it ever binds (measured: a built app crashed at \
boot exactly this way). Either open the connection IN the thread that uses it, open one connection \
per thread, or pass check_same_thread=False AND guard every access with one lock. ThreadingHTTPServer \
handlers run on N threads — one shared unguarded connection is a race even when it does not throw.
15. http.server request handlers do their WORK inside __init__: BaseHTTPRequestHandler.__init__ calls \
handle() and do_GET before returning, so any attribute attached 'after construction' (a patched \
__init__ that sets self.store AFTER calling the original, an instance attribute set post-construction) \
does not exist when do_GET runs — AttributeError on EVERY request while the server 'runs' fine \
(measured: an app served nothing but 500s for exactly this). Hand state to handlers via CLASS \
attributes, functools.partial on the handler class, or attributes on the SERVER object read as \
self.server.store — set BEFORE serve_forever, never patched in after.
16. http.client.HTTPConnection takes a HOST[:PORT], never a URL: HTTPConnection('http://127.0.0.1:8990') \
treats the whole string as a hostname and fails DNS ('nodename nor servname provided'). Parse the \
base URL first (urllib.parse.urlsplit) and pass host and port separately — or use urllib.request \
which accepts full URLs. Verify the server BINDS and answers before layering features: a server \
that cannot boot makes every other line of the app worthless.
17. `python -m X` boots a PACKAGE X only through X/__main__.py: without that file the invocation fails \
instantly with 'No module named X.__main__' even when the package imports fine and its code is perfect \
(measured: two builds shipped service packages without __main__.py and two of three advertised \
invocations could never boot, zeroing the delivery). EVERY invocation the spec advertises must boot — \
each package form needs its own __main__.py (argparse the documented flags, then start the service), a \
subpackage needs __init__.py at every level, and the proof is running each advertised command and \
watching it bind its port, not reading the code.
18. BIND FIRST, work after: a service's boot deadline applies to LISTENING, never to data readiness. \
Create the server socket, bind, and start serving BEFORE any vendor/network call, initial sync, or \
long init — run those in a background thread started AFTER the socket listens, e.g. \
threading.Thread(target=first_sync, daemon=True).start() right after serve begins (measured: an app \
that attempted its first vendor sync before binding never listened at all while the vendor was \
unreachable, and the delivery scored zero although the code behind the port was fine). The background \
sync loop must tolerate a dead vendor forever — retry on a timer while the server keeps serving \
local data; a dead vendor must never keep the port closed.
19. Browser JavaScript resolves identifiers at EVALUATION, not at load: referencing a name that is defined \
nowhere (a typo, or a rename that missed one reference) throws ReferenceError only when that line RUNS, and \
inside an init/boot function the throw kills EVERY statement after it — later registrations, data loads and \
render-loop starts silently never happen, so one wrong identifier severs the whole feature while the page \
still 'loads' (measured: a built page lost its largest feature exactly this way — a handler registered under \
a name that existed nowhere, and everything after the throw never ran). After ANY rename, update EVERY \
reference to the old name. Before handing off, cross-check each identifier you reference against a \
definition or import in the files you ship, and load the served page: it is not done until the browser \
console shows ZERO errors.
20. The response SHAPE is the contract: an API handler that does the right work but omits the documented \
response fields for its exact route is a defect no client, test or gate downstream can verify — correct \
behavior with a wrong-shaped reply reads as broken from the outside (measured: a set of handlers behaved \
correctly and answered undocumented shapes, and the findings persisted round after round because nothing \
could confirm the contract). Re-read the spec's endpoint table for the routes YOU own and return the \
documented fields from the handler itself — every documented key, exact names, not a subset. Error paths \
answer the same contract: an auth failure or a validation error still returns the documented JSON envelope \
with its documented fields, never a bare status code or an improvised error shape.\
";

/// Triggers are deliberately UNAMBIGUOUS, not merely topical. A first cut used bare words like "page",
/// "round", "index" and "year" — and "Render a static about page" then pulled in pagination trivia. A false
/// trigger is not free: it spends the author's attention on cron facts during a CSS task, which is the exact
/// dilution this retrieval exists to avoid. When in doubt, DO NOT match — the cost of missing a fact is one
/// review finding; the cost of crying wolf on every task is that the author stops reading.
const PITFALL_TRIGGERS: &[&[&str]] = &[
    &["cron", "crontab", "day-of-week", "day of week", "weekday"],
    &[
        "timezone",
        "time zone",
        "utc",
        "dst",
        "daylight",
        "datetime",
        "zoneinfo",
        "epoch",
        "timestamp",
        "unix time",
    ],
    &[
        "0-based",
        "1-based",
        "zero-indexed",
        "one-indexed",
        "day-of-month",
        "iso week",
        "nth ",
    ],
    &[
        "end-exclusive",
        "end-inclusive",
        "inclusive",
        "exclusive",
        "between",
        "range(",
        "time bucket",
    ],
    &[
        "money",
        "currency",
        "price",
        "cents",
        "invoice",
        "decimal",
        "rounding",
        "floating point",
        "float",
        "amount_minor",
        "minor unit",
        "total",
        "sum",
    ],
    &[
        "off-by-one",
        "fencepost",
        "pagination",
        "page number",
        "page size",
        "offset",
    ],
    &["leap year", "february", "365 days", "days in a month"],
    &[
        "unicode",
        "utf-8",
        "codepoint",
        "code point",
        "grapheme",
        "casefold",
        "truncate",
        "encode(",
    ],
    &[
        "integer division",
        "floor division",
        "modulo",
        "remainder",
        "//",
    ],
    &[
        "mutable default",
        "default arg",
        "lexicographic",
        "sorted(",
        "sort order",
    ],
    &[
        "week start",
        "business day",
        "weekend",
        "holiday",
        "percentage",
        "basis point",
    ],
    // Deliberately the LIBRARY/SCHEME names, never the domain words. MEASURED against the four archived
    // 3-node plans before being written: "vendor" hits 11 of 16 tasks and "api" 6 — the exact crying-wolf
    // the note above this const forbids — while this row reaches the module that actually owns the HTTP
    // client (`meridian`) in EVERY cell tested and only ~25% of tasks overall.
    &[
        "http://",
        "https://",
        "urllib",
        "urlopen",
        "httpx",
        "requests.get",
        "requests.post",
        "axios",
        "fetch(",
        "http client",
        "api client",
    ],
    &[
        "etag",
        "if-none-match",
        "conditional request",
        "next_cursor",
        "fetch_all",
    ],
    &[
        "sqlite",
        "sqlite3",
        "check_same_thread",
        "threadinghttpserver",
    ],
    &[
        "basehttprequesthandler",
        "http.server",
        "do_get",
        "do_post",
        "httpserver",
        "request handler",
    ],
    &[
        "httpconnection",
        "http.client",
        "meridianclient",
        "vendor client",
    ],
    &["python -m", "python3 -m", "__main__"],
    &[
        "serve_forever",
        "listening within",
        "binds",
        "bind 127",
        "first sync",
        "--port",
    ],
    // Item 19 (ReferenceError severs a boot function). 'render' is REJECTED — it matches
    // template-rendering backend tasks and the static-page fixture below; '.js' is rejected as a
    // substring of '.json'. Platform words and browser API names only.
    &[
        "javascript",
        "frontend",
        "front-end",
        "browser",
        "canvas",
        "webgl",
        "addeventlistener",
        "queryselector",
        "getelementbyid",
        "requestanimationframe",
    ],
    // Item 20 (the response shape IS the contract). 'api' is REJECTED — measured at 6 of 16
    // archived task specs (the crying-wolf the note above forbids) and the api-schema fixture
    // must stay silent. Handler-surface words only.
    &[
        "endpoint",
        "handler",
        "route",
        "webhook",
        "response envelope",
    ],
];

/// The DOMAIN_PITFALLS items relevant to this task's text, or None when nothing matches.
///
/// Splits the SAME const the reviewer and skeptic read — there is no second copy to drift out of sync. If
/// an item is added to DOMAIN_PITFALLS without a trigger row, `pitfall_items_match_triggers` fails loudly
/// rather than silently making the new fact unreachable to the author.
pub(super) fn relevant_pitfalls(task_text: &str) -> Option<String> {
    let hay = task_text.to_lowercase();
    let hits: Vec<String> = pitfall_items()
        .into_iter()
        .zip(PITFALL_TRIGGERS.iter())
        .filter(|(_, triggers)| triggers.iter().any(|t| hay.contains(*t)))
        .map(|(item, _)| item)
        .collect();
    if hits.is_empty() {
        return None;
    }
    Some(hits.join("\n"))
}

/// DOMAIN_PITFALLS split back into its numbered items ("1. …", "2. …"). The const is authored as one
/// block with `\`-continued lines, so an item starts only where a line begins with `<n>. `; every other
/// line is a continuation of the item above it.
///
/// Builds owned Strings rather than slicing the const by byte offset — the library is prose with em-dashes
/// and other multi-byte characters, and a byte slice that lands mid-character panics.
fn pitfall_items() -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for line in DOMAIN_PITFALLS.lines() {
        let starts_item = line
            .split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if starts_item {
            items.push(line.to_string());
        } else if let Some(last) = items.last_mut() {
            last.push(' ');
            last.push_str(line.trim());
        }
    }
    for i in items.iter_mut() {
        *i = i.trim_end().to_string();
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parser must find EVERY item and lose NO text — it feeds a prompt, so a silently dropped fact is
    /// a fact the author never learns. Also pins the trigger table to the library: adding a pitfall without
    /// a trigger row makes it unreachable to the author, and that must fail loudly here.
    #[test]
    fn pitfall_items_match_triggers() {
        let items = pitfall_items();
        assert_eq!(
            items.len(),
            PITFALL_TRIGGERS.len(),
            "every DOMAIN_PITFALLS item needs a trigger row (or the author can never receive it)"
        );
        for (i, item) in items.iter().enumerate() {
            assert!(
                item.starts_with(&format!("{}. ", i + 1)),
                "item {} out of order: {:.40}",
                i + 1,
                item
            );
            assert!(
                !PITFALL_TRIGGERS[i].is_empty(),
                "item {} has no triggers",
                i + 1
            );
        }
        // No text may be lost between the const and the items the author would see.
        let rejoined: String = items.join("\n");
        let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(
            norm(&rejoined),
            norm(DOMAIN_PITFALLS),
            "the split lost or mangled library text"
        );
    }

    #[test]
    fn relevant_pitfalls_retrieves_only_what_the_task_is_about() {
        // A cron task gets the cron fact (the measured off-by-one class) and NOT money/unicode noise.
        let cron =
            relevant_pitfalls("Build a cron scheduler that parses a crontab day-of-week field")
                .expect("cron task must retrieve the cron pitfall");
        assert!(cron.contains("CRON day-of-week is 0=Sunday"));
        assert!(
            !cron.contains("MUST NOT be a binary float"),
            "money trivia must not ride along"
        );

        // An invoice task gets money, not cron.
        let money = relevant_pitfalls("Compute the invoice total price in currency with rounding")
            .expect("money task must retrieve the money pitfall");
        assert!(money.contains("MUST NOT be a binary float"));
        assert!(
            !money.contains("CRON day-of-week"),
            "cron trivia must not ride along"
        );

        // Retrieval is case-insensitive (specs are prose, not lowercase).
        assert!(relevant_pitfalls("Handle TIMEZONE and DST correctly").is_some());

        // A task about none of it gets NOTHING — silence beats diluting the prompt.
        assert!(
            relevant_pitfalls("Render a static about page with a logo").is_none(),
            "an unrelated task must receive no pitfalls at all"
        );
    }

    /// The two r5-measured defect classes, taught general: a referenced-but-never-defined
    /// identifier severing a browser boot function, and a handler answering a wrong-shaped
    /// response. Each lesson must REACH a task shaped like the one that needed it, and neither
    /// may spray onto the cron/money fixtures above.
    #[test]
    fn the_reference_and_response_shape_lessons_reach_their_authors() {
        let viz = relevant_pitfalls(
            "Implement the 3D field in raw WebGL: instanced columns on a canvas, GPU pick buffer, \
             orbit camera. web/field.js",
        )
        .expect("a browser-JS task must retrieve the ReferenceError pitfall");
        assert!(viz.contains("throws ReferenceError"));
        assert!(
            !viz.contains("CRON day-of-week"),
            "cron trivia must not ride along"
        );

        let handlers = relevant_pitfalls(
            "Implement the POST endpoint handlers so each route returns the documented response \
             fields on success and error paths",
        )
        .expect("a handler task must retrieve the response-shape pitfall");
        assert!(handlers.contains("The response SHAPE is the contract"));

        // Neither lesson leaks onto the fixtures the library already guards.
        let cron =
            relevant_pitfalls("Build a cron scheduler that parses a crontab day-of-week field")
                .unwrap();
        assert!(!cron.contains("ReferenceError"));
        assert!(!cron.contains("response SHAPE"));
    }
}
