#!/usr/bin/env python3
"""Exactly six unscored Goose tool-loop calls, using only the frozen INSTALLED harness.

This is provider/cache support qualification, not a benchmark entrant: no scorer, capability
score or publication is run. Each normal and auxiliary generation is billed by the controller.
"""
import argparse
from decimal import Decimal
from dataclasses import replace
import json
import os
from pathlib import Path
import shutil
import shlex
import sqlite3
import sys

from ops import Hold, catalog, ensure_config, load_installed, read, verify_installed, write, live_credit_evidence

TASK = r'''const fs=require('node:fs'),cp=require('node:child_process'),path=require('node:path'),crypto=require('node:crypto');
const step=Number(process.argv[2]); if(!Number.isInteger(step)||step<1||step>5) throw Error('step 1..5 required');
const digest=x=>crypto.createHash('sha256').update(x).digest('hex');
let result;
if(step===1){const raw=fs.readFileSync('manifest.yml','utf8'); result={manifestSha:digest(raw),manifestBytes:Buffer.byteLength(raw),moduleLines:raw.split('\n').filter(l=>/^  [a-z].*:/.test(l))};}
if(step===2)result=JSON.parse(fs.readFileSync(path.join(process.env.FORGE_KIT,'KIT.json'),'utf8'));
if(step===3){const raw=fs.readFileSync('RATE-MODEL.json','utf8'); result={rateSha:digest(raw),rate:JSON.parse(raw)};}
if(step===4||step===5){const output=cp.execFileSync(process.execPath,[path.join(process.env.FORGE_KIT,'bin/forge-dev.cjs'),step===4?'users':'kvs'],{encoding:'utf8',maxBuffer:16*1024*1024}); result={operation:step===4?'users':'kvs',outputSha:digest(output),outputBytes:Buffer.byteLength(output)}; if(step===4)result.users=output;}
const resultSha=digest(JSON.stringify(result));const receipt={step,resultSha,result};
fs.writeFileSync(`cache-step-${step}.json`,JSON.stringify(receipt));console.log(JSON.stringify(receipt));
'''


def tool_history(runtime, node):
    databases = list((Path(runtime) / 'goose').rglob('sessions.db'))
    if len(databases) != 1:
        raise Hold('Qualification has no unique isolated Goose history')
    connection = sqlite3.connect('file:' + str(databases[0]) + '?mode=ro', uri=True)
    try:
        rows = connection.execute('SELECT id,role,content_json FROM messages ORDER BY id').fetchall()
    finally:
        connection.close()
    requests, responses, final_text = [], {}, []
    for _, role, raw in rows:
        for content in json.loads(raw):
            kind = content.get('type')
            if kind == 'toolRequest':
                call = content.get('toolCall', {})
                if call.get('status') != 'success':
                    raise Hold('Goose history contains a rejected tool request')
                value = call['value']
                requests.append({'id': content['id'], 'name': value['name'], 'arguments': value.get('arguments')})
            elif kind == 'toolResponse':
                result = content.get('toolResult', {})
                responses[content['id']] = result.get('status') == 'success' and not result.get('value', {}).get('isError', False)
            elif kind == 'text' and role == 'assistant':
                final_text.append(content.get('text', ''))
    commands = [f'{shlex.quote(node)} cache-probe-task.cjs {step}' for step in range(1, 6)]
    if len(requests) != 5 or any(request['name'] != 'developer__shell'
                              or request['arguments'] != {'command': command}
                              or not responses.get(request['id'])
                              for request, command in zip(requests, commands)):
        raise Hold('Actual Goose history does not contain five separate successful ordered tool calls')
    return {'tool_requests': requests, 'matching_successful_responses': len(requests),
            'assistant_final': final_text[-1] if final_text else None}


def run(config, unit, workdir):
    modules = load_installed(config['payload'], config['engine'])
    verify_installed(config, modules)
    _, _, _, _, run_build = modules
    import isolated_tiers, forge2_kit, forge2_site
    row = catalog()[0].get(unit['model'])
    if row is None:
        raise Hold('Exact requested catalog ID is unavailable; no alias was used')
    snapshot = ensure_config(run_build.entrant_config('openrouter'), unit['model'])
    if workdir.exists():
        raise Hold('Qualification workspace already exists; never replay a paid unit')
    os.environ.update(BENCH_FORGE20='1', GOOSE_SWARM_BENCHMARK='1', BENCH_MAX_USD=unit['wallet'],
                      GOOSE_SWARM_RENDER_NODE=config['node'])
    isolated_tiers.active = lambda: replace(isolated_tiers.FORGE20, call_budget=6)
    credentials = run_build.cloud_env('openrouter', snapshot)
    effort = run_build.pinned_reasoning_effort(isolated_tiers.FORGE20, credentials)
    credentials['GOOSE_THINKING_EFFORT'] = effort['value']
    limits = run_build.provider_model_limits('openrouter', unit['model'], credentials)
    credentials.update({key: value for key, value in limits.items() if key.startswith('GOOSE_')})
    # A title is still billed; its request has no developer tools and is recorded separately.
    credentials['BENCH_MAX_USD'] = unit['wallet']
    kit = forge2_kit.ensure()
    workdir.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    shutil.copytree(Path(config['payload']) / 'forge2/starter', workdir,
                    ignore=shutil.ignore_patterns(*run_build.KIT_TREE_EXCLUDES))
    os.chmod(workdir, 0o700)
    run_build.clone_kit_modules({'dir': kit['kit_dir']}, workdir)
    (workdir / 'cache-probe-task.cjs').write_text(TASK)
    site = forge2_site.serve(0, workdir / 'dev-site-trace.jsonl')
    try:
        port = int(site.site_url.rsplit(':', 1)[1])
        credentials.update(FORGE_KIT=kit['kit_dir'], FORGE_SITE_URL=site.url)
        prefix = []
        for name, source in isolated_tiers.FORGE20.public:
            text = run_build.render_public_contract((Path(config['payload']) / source).read_text(), port, forge2_site)
            (workdir / name).write_text(text)
            prefix.append(f'\nReference {name}:\n{text}')
        commands = '\n'.join(f'{shlex.quote(config["node"])} cache-probe-task.cjs {step}' for step in range(1, 6))
        prompt = ('This is an UNSCORED provider/cache qualification inside Goose. Your entire task is the '
                  'six-call procedure at the end. The Forge contract below is reference context; do not implement '
                  'the app, modify it, run tests, browse, or add any other tool calls.\n' + ''.join(prefix) +
                  '\n\nQualification procedure: make exactly one developer__shell call per turn for each '
                  'of these five commands, in order. Pass only the command argument exactly as printed. '
                  'Wait for each actual tool result before the next call.\n' + commands +
                  '\nOn your sixth model call, make NO tool call. Answer QUALIFICATION_COMPLETE followed '
                  'by the five resultSha values returned by the commands, in order. This is not capability scoring.')
        run_build.build_prompt = lambda _port: prompt
        write(workdir.parent / 'probe-launch.json', {'model': unit['model'], 'wallet': unit['wallet'],
              'taskFingerprint': config['taskFingerprint'], 'engine_sha256': config['engine_sha256'],
              'kind': 'unscored-provider-cache-qualification', 'max_normal_calls': 6,
              'dev_seed': site.dev_seed, 'reasoning_effort': effort, 'model_limits': limits})
        current_credit = live_credit_evidence(modules)
        write(workdir.parent / 'probe-credit-readback.json', current_credit)
        if Decimal(current_credit['effective_remaining_usd']) < Decimal(unit['wallet']):
            raise Hold('Current credits shrank during provider qualification preparation; no paid call started')
        agent = run_build.invoke('cache-qualification', workdir, port, credentials, 0,
                                 provider='openrouter', model=unit['model'], snapshot=snapshot)
        runtime = workdir.parent / ('.' + workdir.name + '-runtime')
        history = tool_history(runtime, config['node'])
        receipts = [read(workdir / f'cache-step-{step}.json') for step in range(1, 6)]
        if not history['assistant_final'] or 'QUALIFICATION_COMPLETE' not in history['assistant_final'] \
                or any(r['resultSha'] not in history['assistant_final'] for r in receipts):
            raise Hold('The sixth actual Goose reply did not report the observed five tool results')
        modules[2].validate_unchanged(Path(config['payload']), {'taskFingerprint': config['taskFingerprint'], 'buildId': unit['key']})
        write(workdir.parent / 'qualification.json', {'status': 'tool-loop-observed', 'model': unit['model'],
              'history': history, 'tool_receipts': receipts, 'agent': {k: v for k, v in agent.items() if k != 'tail'}})
    finally:
        site.shutdown()
        site.server_close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('config', type=Path)
    parser.add_argument('unit', type=Path)
    parser.add_argument('workdir', type=Path)
    parser.add_argument('--execute-paid', action='store_true')
    args = parser.parse_args()
    if not args.execute_paid:
        parser.error('--execute-paid is required after freeze')
    try:
        run(read(args.config), read(args.unit), args.workdir)
    except Exception as error:
        # Never serialize configuration/credentials or raw provider errors into the public status.
        write(args.workdir.parent / 'qualification-failure.json', {'status': 'unscored',
              'reason': str(error) if isinstance(error, Hold) else type(error).__name__})
        raise SystemExit(1)


if __name__ == '__main__':
    main()
