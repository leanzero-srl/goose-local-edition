import argparse
from .store import Store
from .meridian import MeridianClient
from .api import serve
p=argparse.ArgumentParser();p.add_argument('--db',required=True);p.add_argument('--port',type=int,default=8080);a=p.parse_args();serve(a.port,Store(a.db),MeridianClient('http://127.0.0.1:9003','sk_test_meridian'))
