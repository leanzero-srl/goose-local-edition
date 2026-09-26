# goose distributed tensor rank: where this rank's generation loop is, in the rank's own words.
# Pure stdlib; concatenated after rank_batch.py, before rank_wrapper.py (which feeds it), and read by
# rank_env.py's reporter, which prints the published snapshot as `GOOSE_RANK_STATE` beside every
# `GOOSE_RANK_MEM` — into the rank's durable log (goosed's rank_log.rs, per rank, on the Mac whose
# goosed reads the rank: a Link rank's on the peer).
#
# Q-114 (2026-09-26, 3.0.45, tensor split over JACCL, 27B): mid-generation all progress stopped; rank
# 0 spun in a collective at 97% CPU, rank 1 sat at 0% CPU, and goosed SIGTERMed both 20 s later.
# Whether rank 1 was parked on the doorbell (it believed the batch was over), blocked writing its
# output, or stuck inside MLX could not be told: its output lived only in goosed's memory and it
# printed nothing about its loop. Every rank now publishes, at each step of its loop:
# - `steps`: its own `_next_request` count (rank 0's is /goose/progress; a worker's was never read);
# - `at`: where the loop is — `poll` (rank 0 waiting on its request queue), `doorbell` (a worker
#   parked in recv(1) for rank 0's ring), `share` (in the request share's collective), `batch` (the
#   batch step: the model's collectives), `idle` (nothing shared, nothing running);
# - `mode`: `busy` while mlx_lm runs a batch (`timeout` None), `idle` otherwise — the decision
#   the doorbell keys on; `rings`: rings rank 0 sent / a worker received; `rows`, `width`: the batch;
# - per generating row, its token trail (`TokenTrail`): two ranks that sampled the same tokens hold
#   the same trails, so a batch that ended on one rank only is visible as a trail that stops there.
import zlib


class TokenTrail:
    """One row's generated tokens, folded: the count, a CRC-32 over them, and the CRC at every
    power-of-two count. The first checkpoint two ranks' trails disagree at brackets the token where
    their samples diverged within a factor of two, whatever the counts each rank stopped at."""

    def __init__(self):
        self.generated = 0
        self.crc = 0
        self.checkpoints = {}

    def fold(self, token):
        self.generated += 1
        self.crc = zlib.crc32(int(token).to_bytes(4, "little"), self.crc)
        if self.generated & (self.generated - 1) == 0:
            self.checkpoints[str(self.generated)] = self.crc

    def report(self, uid, **more):
        return {
            "uid": uid,
            "generated": self.generated,
            "crc": self.crc,
            "checkpoints": dict(self.checkpoints),
            **more,
        }


class LoopState:
    """The generation loop's position, written by the generation thread only. `publish` hands the
    reporter a fresh dict each time (never one it mutates later), so the reporter thread reads a
    whole snapshot."""

    def __init__(self, rank):
        self.rank = rank
        self.steps = 0
        self.at = "starting"
        self.mode = None
        self.rings = 0
        self.rows = 0
        self.width = 0
        self.held = 0
        self.trails = {}
        self.ended = None

    def new_batch(self):
        self.trails = {}

    def fold(self, uid, token, finish_reason):
        trail = self.trails.setdefault(uid, TokenTrail())
        trail.fold(token)
        if finish_reason is not None:
            self.ended = trail.report(uid, how=finish_reason)
            del self.trails[uid]

    def drop(self, uids):
        for uid in uids:
            trail = self.trails.pop(uid, None)
            if trail is not None:
                self.ended = trail.report(uid, how="removed")

    def publish(self, at):
        self.at = at
        published_state[0] = {
            "rank": self.rank,
            "steps": self.steps,
            "at": at,
            "mode": self.mode,
            "rings": self.rings,
            "rows": self.rows,
            "width": self.width,
            "held": self.held,
            "trails": [trail.report(uid) for uid, trail in self.trails.items()],
            "ended": self.ended,
        }
