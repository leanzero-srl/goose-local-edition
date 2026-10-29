"""Feed notarytool's secure prompt only after its terminal disables echo."""
import errno, fcntl, os, pty, selectors, subprocess, sys, termios, time

secret = os.environ.get('APPLE_ID_PASSWORD')
if not secret or '\n' in secret or '\r' in secret:
    raise SystemExit('Missing or invalid local notarization credential')
env = {k:v for k,v in os.environ.items() if k not in ('APPLE_ID_PASSWORD','APPLE_ID','APPLE_TEAM_ID')}
master, slave = pty.openpty()
def controlling_terminal():
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)
process = subprocess.Popen(sys.argv[1:], stdin=slave, stdout=subprocess.PIPE, stderr=slave,
                           env=env, start_new_session=True, preexec_fn=controlling_terminal)
os.close(slave)
selector = selectors.DefaultSelector()
selector.register(master, selectors.EVENT_READ, 'prompt')
selector.register(process.stdout, selectors.EVENT_READ, 'json')
prompt = bytearray(); output = bytearray(); sent = False
started = time.monotonic()
try:
    while selector.get_map():
        for key, _ in selector.select(0.1):
            try: data = os.read(key.fd, 65536)
            except OSError as error:
                if error.errno != errno.EIO: raise
                data = b''
            if not data:
                selector.unregister(key.fileobj); continue
            if key.data == 'json': output.extend(data)
            else: prompt.extend(data)
        if not sent and b'password' in prompt.lower():
            if not termios.tcgetattr(master)[3] & termios.ECHO:
                os.write(master, secret.encode()+b'\n'); sent = True
        if not sent and time.monotonic()-started > 30:
            raise RuntimeError('No secure password prompt; credential was not sent')
    status = process.wait()
    def scrub(value):
        return value.decode(errors='replace').replace(secret, '[redacted]')
    if status:
        sys.stderr.write(scrub(prompt)+scrub(output)); raise SystemExit(status)
    if not sent:
        raise RuntimeError('Command completed without expected secure authentication prompt')
    sys.stdout.write(scrub(output))
finally:
    if process.poll() is None:
        process.terminate(); process.wait()
    os.close(master)
