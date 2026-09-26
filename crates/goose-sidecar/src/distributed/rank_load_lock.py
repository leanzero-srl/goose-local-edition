# goose's machine-wide load lock, taken by a rank: one model load at a time per Mac (Q-106: several
# goose processes loaded 27B engines on one MacBook at once and wedged its GPU until a reboot).
#
# Embedded FIRST in every goose rank program (distributed/launch.rs): rank_env.py takes the lock
# before MLX is imported and releases it when the rank reports RANK_CAPS — its weights are in. It is
# the lock goose's single engine takes (goose-sidecar machine.rs): an exclusive flock on
# `~/.local/state/goose/mlx-load.lock` under the account's home, whose text is the holder's record
# (`key=value` lines: pid, started, since, what, [port], [group], [model]). A rank:
# - takes the lock when nobody holds it; a recorded holder is displaced only when PROVEN gone (no
#   such pid, a zombie, or the pid started at another time — a reused pid);
# - joins the hold when a rank of the SAME split holds it (`group`): a split's ranks on one Mac are
#   one load, and refusing a sibling would leave the group's collectives waiting for it;
# - otherwise refuses in words (who holds the Mac; wait for it or stop it) and exits 1 — the line
#   `GOOSE_RANK_LOAD_REFUSED <json>` and the refusal on stderr land in the rank's tail.
import ctypes
import fcntl
import json
import os
import pwd
import struct
import time

LOAD_LOCK_RELATIVE = ".local/state/goose/mlx-load.lock"
_load_lock = {"fd": None}


def load_lock_path():
    # The account database's home, never $HOME: the lock is the Mac's, whatever a launcher exported.
    return os.path.join(pwd.getpwuid(os.getuid()).pw_dir, LOAD_LOCK_RELATIVE)


def process_start(pid):
    """(start in unix seconds, zombie) — libproc's PROC_PIDTBSDINFO, the figure goose reads through
    sysinfo (`pbi_start_tvsec`, offset 120 of the 136-byte proc_bsdinfo; `pbi_status` at 4, SZOMB
    5) — or None when the kernel names no such process to this account."""
    libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
    info = ctypes.create_string_buffer(136)
    if libproc.proc_pidinfo(pid, 3, ctypes.c_uint64(0), info, 136) != 136:
        return None
    return struct.unpack_from("<Q", info, 120)[0], struct.unpack_from("<I", info, 4)[0] == 5


def read_record(fd):
    os.lseek(fd, 0, os.SEEK_SET)
    chunks = []
    while True:
        chunk = os.read(fd, 65536)
        if not chunk:
            break
        chunks.append(chunk)
    text = b"".join(chunks).decode("utf-8", "replace")
    return dict(line.split("=", 1) for line in text.splitlines() if "=" in line)


def holder_alive(holder):
    # The kernel answers no process info for a zombie, so a pid this account may signal but not
    # read has exited; another account's pid (the signal refused) is never taken for gone.
    pid = int(holder["pid"])
    signal_refused = False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False, f"no process has pid {pid}"
    except PermissionError:
        signal_refused = True
    start = process_start(pid)
    if start is None:
        if signal_refused:
            return True, None
        return False, f"pid {pid} has exited (the kernel answers no process info for it: a zombie)"
    started, zombie = start
    if zombie:
        return False, f"pid {pid} has exited (a zombie holds no files)"
    if started != int(holder["started"]):
        return False, f"pid {pid} is now another process (it started at {started}, the holder at {holder['started']})"
    return True, None


def elapsed_words(seconds):
    if seconds < 60:
        return f"{seconds}s"
    if seconds < 3600:
        return f"{seconds // 60}m {seconds % 60}s"
    return f"{seconds // 3600}h {(seconds % 3600) // 60}m"


def refuse(path, holder):
    if "pid" in holder and "started" in holder:
        alive, proof = holder_alive(holder)
    else:
        alive, proof = None, None
    if alive is None:
        message = (
            f"this Mac's load lock ({path}) is held by a process whose record cannot be read "
            f"({holder!r}) — `lsof {path}` names it"
        )
    elif alive:
        since = int(holder.get("since", "0"))
        message = (
            f"another model is loading on this Mac: pid {holder['pid']} — {holder.get('what', '')} — "
            f"loading for {elapsed_words(max(0, int(time.time()) - since))}. One model loads at a "
            "time per Mac — two loads at once can wedge its GPU. Wait for it to finish, or stop it "
            "first, then start the split again"
        )
    else:
        message = (
            f"this Mac's load lock ({path}) is held, but its recorded holder is gone ({proof}): a "
            f"process that inherited the lock still holds it — `lsof {path}` names it"
        )
    print(
        "GOOSE_RANK_LOAD_REFUSED "
        + json.dumps({"holder": holder, "holderAlive": alive, "lock": path, "message": message}),
        flush=True,
    )
    raise SystemExit(f"goose rank: {message}")


def take_load_lock(path, group, what, model=None):
    """The lock's fd when this rank took it; None when a rank of its own split holds it. `model`:
    the id a refusal names in plain words (`model=` in the record, as goose's single engine writes)."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o644)
    while True:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except BlockingIOError:
            holder = read_record(fd)
            if not holder:
                # The holder is between its flock and its record (or its release is between
                # emptying the record and unlocking): one grace tick, then look again.
                time.sleep(0.1)
                continue
            if holder.get("group") == group:
                os.close(fd)
                return None
            os.close(fd)
            refuse(path, holder)
    if os.fstat(fd).st_ino != os.stat(path).st_ino:
        os.close(fd)
        raise SystemExit(f"goose rank: the load lock {path} was replaced while it was being taken")
    previous = read_record(fd)
    if previous and previous.get("pid") != str(os.getpid()):
        if "pid" in previous and "started" in previous:
            alive, _ = holder_alive(previous)
        else:
            alive = False
        if alive:
            os.close(fd)
            refuse(path, previous)
    start = process_start(os.getpid())
    if start is None:
        os.close(fd)
        raise SystemExit("goose rank: the kernel does not name this rank's own start time")
    record = (
        f"pid={os.getpid()}\nstarted={start[0]}\nsince={int(time.time())}\n"
        f"what={' '.join(what.splitlines())}\ngroup={group}\n"
    )
    if model:
        record += f"model={' '.join(str(model).splitlines())}\n"
    os.ftruncate(fd, 0)
    os.lseek(fd, 0, os.SEEK_SET)
    os.write(fd, record.encode())
    os.fsync(fd)
    _load_lock["fd"] = fd
    return fd


def release_load_lock():
    # Empty the record, then unlock: a released lock never shows a holder. LOCK_UN releases the
    # lock for every copy of the descriptor (a forked child's included), not only this one.
    fd = _load_lock["fd"]
    if fd is None:
        return
    _load_lock["fd"] = None
    os.ftruncate(fd, 0)
    fcntl.flock(fd, fcntl.LOCK_UN)
    os.close(fd)
