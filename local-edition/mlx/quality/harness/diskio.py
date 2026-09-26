#!/usr/bin/env python3
"""diskio.py <pid|name> [seconds] — bytes a process wrote/read over a window, from proc_pid_rusage
(ri_diskio_byteswritten/read, RUSAGE_INFO_V2). Q-115: macOS flagged goosed for 137 GB dirtied in 21 min."""
import ctypes, subprocess, sys, time
lib = ctypes.CDLL('/usr/lib/libproc.dylib')
class RU(ctypes.Structure):
    _fields_ = [('uuid', ctypes.c_uint8 * 16)] + [(n, ctypes.c_uint64) for n in (
        'user', 'system', 'pkg_idle_wkups', 'interrupt_wkups', 'pageins', 'wired', 'resident', 'phys_footprint',
        'start', 'exit', 'child_user', 'child_system', 'child_pkg_idle', 'child_interrupt', 'child_pageins',
        'child_elapsed', 'diskio_read', 'diskio_written')]
arg = sys.argv[1]
pid = int(arg) if arg.isdigit() else int(subprocess.check_output(['pgrep', '-n', '-f', arg]).split()[0])
def sample():
    r = RU(); assert lib.proc_pid_rusage(pid, 2, ctypes.byref(r)) == 0, 'proc_pid_rusage failed'
    return r.diskio_read, r.diskio_written
secs = float(sys.argv[2]) if len(sys.argv) > 2 else 60
r0, w0 = sample(); time.sleep(secs); r1, w1 = sample()
print(f'pid {pid}: wrote {(w1 - w0) / 1e6:.1f} MB ({(w1 - w0) / 1e6 / secs:.2f} MB/s), read {(r1 - r0) / 1e6:.1f} MB over {secs:.0f}s; lifetime written {w1 / 1e9:.1f} GB')
