import getpass, json, os, sys
if len(sys.argv)>1:
 print('App-specific password:', file=sys.stderr, flush=True)
 import select
 assert not select.select([sys.stdin],[],[],0.3)[0], 'Credential sent with echo enabled'
 sys.exit(7)
password=getpass.getpass('App-specific password: ')
assert password == 'fixture-private-password'
assert 'APPLE_ID_PASSWORD' not in os.environ
assert password not in '\0'.join(sys.argv)
print(json.dumps({'securePrompt':'passed'}))
