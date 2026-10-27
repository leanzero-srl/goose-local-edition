"""Fingerprint bytes, modes, symlinks and xattrs without following bundle links."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import stat
import sys

libc = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
libc.listxattr.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
libc.listxattr.restype = ctypes.c_ssize_t
libc.getxattr.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_void_p,
                         ctypes.c_size_t, ctypes.c_uint32, ctypes.c_int]
libc.getxattr.restype = ctypes.c_ssize_t


def read_xattr(call, label):
    length = call(None, 0)
    if length < 0:
        raise OSError(ctypes.get_errno(), label)
    buf = ctypes.create_string_buffer(length)
    actual = call(buf, length)
    if actual < 0:
        raise OSError(ctypes.get_errno(), label)
    return buf.raw[:actual]


def attributes(file):
    encoded = os.fsencode(file)
    names = read_xattr(lambda buf, size: libc.listxattr(encoded, buf, size, 1), str(file))
    result = {}
    for name in names.split(b'\0'):
        if name:
            value = read_xattr(
                lambda buf, size: libc.getxattr(encoded, name, buf, size, 0, 1), str(file))
            result[os.fsdecode(name)] = hashlib.sha256(value).hexdigest()
    return result


def fingerprint(root):
    root = Path(root).resolve(strict=True)
    entries = {}
    for directory, dirs, files in os.walk(root, followlinks=False):
        paths = [Path(directory)] + [Path(directory) / name for name in files]
        paths += [Path(directory) / name for name in dirs if (Path(directory) / name).is_symlink()]
        for file in paths:
            info = file.lstat()
            entry = {'mode': stat.S_IMODE(info.st_mode), 'xattrs': attributes(file)}
            if stat.S_ISLNK(info.st_mode):
                target = file.resolve(strict=True)
                if root not in target.parents:
                    raise ValueError('Escaping symlink: ' + str(file))
                entry.update(kind='symlink', target=os.readlink(file))
            elif stat.S_ISREG(info.st_mode):
                digest = hashlib.sha256()
                with file.open('rb') as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                        digest.update(chunk)
                entry.update(kind='file', sha256=digest.hexdigest(), bytes=info.st_size)
            elif stat.S_ISDIR(info.st_mode):
                entry.update(kind='directory')
            else:
                raise ValueError('Unsupported bundle entry: ' + str(file))
            entries[str(file.relative_to(root))] = entry
    return entries


if __name__ == '__main__':
    with open(sys.argv[2], 'w') as output:
        json.dump(fingerprint(sys.argv[1]), output, sort_keys=True, indent=2)
