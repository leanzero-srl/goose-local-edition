"""Real HTTP/restart mutation controls; browser checks are deliberately excluded."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import score_sb8 as scorer
import vendor_service_v4 as vendor


class TransactionChecks(unittest.TestCase):
    def collect(self, mutation=None):
        with tempfile.TemporaryDirectory(prefix='sb8-transactions-') as tmp:
            root=Path(tmp)
            tree=root/'candidate'
            shutil.copytree(Path(__file__).parent/'golden-sb8',tree)
            if mutation:
                file,old,new=mutation
                source=tree/file
                text=source.read_text()
                self.assertEqual(text.count(old),1)
                source.write_text(text.replace(old,new))
            trace=root/'trace.jsonl'
            port=scorer.free_port()
            server=vendor.serve(port,trace,7123)
            original_run=subprocess.run
            def backend_only(args,*a,**kw):
                if any(str(arg).endswith('product_probe_v4.mjs') for arg in args):
                    return subprocess.CompletedProcess(args,0,json.dumps({'checks':[]}), '')
                return original_run(args,*a,**kw)
            try:
                with patch.object(scorer,'_probe_preflight',return_value=None),patch.object(scorer.subprocess,'run',side_effect=backend_only):
                    result=scorer.gather(tree,port,root/'db',trace,seed=7123)
            finally:
                server.shutdown();server.server_close()
            return {row['name']:row for row in result['checks']}

    def test_reference_passes_all_backend_contract_checks(self):
        rows=self.collect()
        for tier in ('A','B'):
            for name in scorer.BACKEND[tier]:
                self.assertEqual(rows.get(name,{}).get('score'),1,(name,rows.get(name)))

    def test_key_order_bug_is_exposed_after_restart(self):
        rows=self.collect(('app.py','sort_keys=True','sort_keys=False'))
        self.assertEqual(rows['durable_reordered_receipt']['score'],0)
        self.assertEqual(rows['durable_receipt']['score'],1)
        self.assertEqual(rows['durable_conflict_precedence']['score'],1)

    def test_semantics_before_revision_bug_is_exposed(self):
        old="    if cmd['revision']!=state['revision']: return 409,{'error':'stale_revision'}"
        new="""    if cmd['revision']!=state['revision']:
        status,value=apply(sc,state,dict(cmd,revision=state['revision']))
        return (status,value) if status==422 else (409,{'error':'stale_revision'})"""
        rows=self.collect(('gantry_oracle.py',old,new))
        for op in ('move','grip','release'):
            self.assertEqual(rows['stale_before_'+op]['score'],0)
        self.assertEqual(rows['stale_revision']['score'],1)
        self.assertEqual(rows['retry_after_stale']['score'],1)


if __name__=='__main__':unittest.main()
