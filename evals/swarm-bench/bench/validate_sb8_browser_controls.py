"""Live browser controls for contract-permitted status errors and independent scoring.

Run with the same GOOSE_SWARM_* browser variables as score_sb8.py.
Uses temporary candidate clones; writes diagnostic receipts into /tmp.
"""
import sys,tempfile,shutil,json
from pathlib import Path
import score_sb8 as s, vendor_service_v4 as v
for kind in ['status-error','missing-selection','missing-selection-and-input']:
 with tempfile.TemporaryDirectory(prefix='sb8-browser-control-') as tmp:
  root=Path(tmp);tree=root/'candidate';shutil.copytree(Path(__file__).parent/'golden-sb8',tree)
  p=tree/'index.html';html=p.read_text()
  if kind=='status-error':html=html.replace("error?'alert':'status'","'status'")
  else:html=html.replace('data-testid="selection"','data-testid="missing-selection"')
  if kind=='missing-selection-and-input':html=html.replace('aria-label="X"','aria-label="Absent X"').replace('<label>X<input','<label>Absent X<input')
  p.write_text(html)
  port=s.free_port();trace=root/'trace';server=v.serve(port,trace,7123)
  try:ctx=s.gather(tree,port,root/'db',trace,seed=7123)
  finally:server.shutdown();server.server_close()
  checks={r['name']:r for r in ctx['checks']}
  if kind=='status-error':assert checks['visible_command_error']['score']==1,checks
  else:
   assert checks['real_3d_pick']['score']==0 and checks['table_selection']['score']==0
   for name in (('ui_move_reaches_backend',) if kind=='missing-selection' else ())+('cargo_tracks_external_backend','orbit_changes_view','camera_is_read_only'):
    assert checks[name]['score']==1,(name,checks[name])
  Path('/tmp/sb8-'+kind+'.json').write_text(json.dumps(ctx,indent=2))
  print(kind,'PASS',flush=True)
