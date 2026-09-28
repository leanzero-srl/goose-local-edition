# goose distributed rank: rank 0's admission (Q-397). goosed's memory watchdog (supervisor.rs
# `monitor`) closes it with POST /goose/admission {"open": false, "reason", "code": "memory_hold"}
# and reopens it with {"open": true} once memory recovered on every node. While it is closed a new
# chat request is answered 503, and the body names the hold by its closer's `code` — never by its
# prose — with the closer's `reason` and `admission`: the path whose GET answers the moment
# admission reopens. A client that recognises the code waits on the engine's own event instead of
# retrying on a clock (#3r turn 18: goose's 3 retries spent ~7 s of a hold that lasted minutes and
# ended the turn). Pure stdlib; concatenated after rank_env.py, before the rank program.

ADMISSION_PATH = "/goose/admission"


class Admission:
    def __init__(self):
        self._changed = threading.Condition()
        self.open = True
        self.reason = None
        self.code = None

    def set(self, body):
        """POST /goose/admission: `open`, and while closed the closer's `reason` and `code`."""
        with self._changed:
            self.open = bool(body.get("open"))
            self.reason = None if self.open else body.get("reason")
            self.code = None if self.open else body.get("code")
            self._changed.notify_all()
        return {"admission_open": self.open}

    def refusal(self):
        """The 503 body for a request that arrives while admission is closed. `code` only when the
        closer named one (an older goosed names none: its clients see today's 503)."""
        error = {
            "message": "goose distributed engine is not admitting new requests: "
            + str(self.reason),
            "type": "server_busy",
            "reason": self.reason,
            "admission": ADMISSION_PATH,
        }
        if self.code is not None:
            error["code"] = self.code
        return {"error": error}

    def wait_open(self):
        """GET /goose/admission: parks the handler's thread until admission is open — at once when
        it already is. No clock: the watchdog's reopen is the only thing that ends the wait, and a
        client that leaves (Stop) or a rank that exits (the split went away) ends it on the
        client's side."""
        with self._changed:
            self._changed.wait_for(lambda: self.open)
        return {"admission_open": True}
