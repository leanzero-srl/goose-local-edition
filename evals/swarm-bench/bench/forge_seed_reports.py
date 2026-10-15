"""Report each recorded fixture separately without changing its aggregate grade."""
import copy


def attach_seed_evidence(aggregate, entries, evidence_seed):
    report = dict(aggregate)
    method = 'mean' if report['tier'] == 'E' else 'worst'
    report['aggregation'] = method
    report['evidence_seed'] = evidence_seed
    report['parts_seed'] = evidence_seed
    report['seed_evidence'] = [
        {'seed': seed, 'score': row['score'], 'detail': row['detail'],
         **({'parts': copy.deepcopy(row['parts'])} if 'parts' in row else {})}
        for seed, row in entries
    ]
    if method == 'mean' and not report.get('unavailable'):
        terms = ' + '.join(f"{row['score']:.4f}" for _, row in entries)
        report['detail'] = (f"mean over {len(entries)} scoring sites: ({terms}) / {len(entries)} = "
                            f"{report['score']:.4f}; each fixture's measurements are listed separately.")
    return report
