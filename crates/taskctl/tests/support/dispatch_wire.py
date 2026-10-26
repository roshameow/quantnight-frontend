"""Synthetic external-process boundary, NOT the original professional owner.

Stdlib only, no sockets, Mongo, subprocesses, tmux or real task configuration.
Executed by OriginalOperationsRuntime in a disposable FakeBash root. Unknown
calls fail. The JSONL transcript is a sticky oracle even if core catches errors.
"""
import json
import os
from pathlib import Path
import sys
import time

root = Path.cwd()
args = sys.argv[1:]
mode = json.loads((root / 'fault.json').read_text()) if (root / 'fault.json').exists() else {}
entry = {'argv': args, 'binding': os.environ.get('QUANTNIGHT_TMUX_BINDING'),
         'tmux': {k: os.environ.get(k) for k in ('TMUX', 'TMUX_PANE', 'TMUX_TMPDIR')},
         'stopRemaining': os.environ.get('QUANTNIGHT_STOP_REMAINING_SECONDS'),
         'stopDeadline': os.environ.get('QUANTNIGHT_STOP_DEADLINE_EPOCH')}
with (root / 'wire.jsonl').open('a') as f:
    f.write(json.dumps(entry) + '\n')

def reply(command, result):
    if mode.get('ownerFail') == command:
        print(json.dumps({'schemaVersion': 1, 'command': command, 'ok': False, 'error': 'injected owner failure'}))
        sys.exit(31)
    print(json.dumps({'schemaVersion': 1, 'command': command, 'ok': True, 'result': result}))

def exact(request, keys):
    assert set(request) == set(keys), (set(request), keys)

def oid(value):
    assert isinstance(value, dict) and set(value) == {'$oid'}
    assert len(value['$oid']) == 24
    return value['$oid']

try:
    if args[0] == 'checker':
        name, remote = args[1:3]
        assert args[3] in ('--require-absent', '--inspect-execution') and args[4] == '--binding'
        bound = json.loads(args[5])
        statepath = root / 'wire_live.json'
        live = json.loads(statepath.read_text()) if statepath.exists() else None
        if mode.get('presence'):
            print('same-name session exists or inspection unknown', file=sys.stderr)
            sys.exit(17)
        if args[3] == '--inspect-execution':
            observed = dict(schemaVersion=1, taskName=name, remote=remote == 'true',
                state='session_absent', consumerRunning=False, identityVerified=False, ownedResidue=False)
            if live:
                assert live['name'] == name and live['binding']['remote'] == (remote == 'true')
                observed.update(state='live', consumerRunning=True, identityVerified=True,
                    taskId=live['binding']['taskId'], launchAttemptId=live['binding']['launchAttemptId'],
                    pid=1002, pidCreatedAt=1700000010.0, tmuxContext=live['context'],
                    configComparison='not_requested' if bound.get('requestedConfig') is None else
                        'equal' if bound['requestedConfig'] == live['config'] else 'different')
            print('QUANTNIGHT_EXECUTION_OBSERVED ' + json.dumps(observed))
        else:
            if live:
                print('existing exact consumer', file=sys.stderr); sys.exit(17)
            print("❌ Session '%s' 不存在 (%s)" % (name, 'remote' if remote == 'true' else 'local'))
    elif args[0] == 'pause':
        name, remote = args[1:3]
        bound = json.loads(entry['binding'])
        assert bound['remote'] == (remote == '--remote')
        assert 0 < float(entry['stopRemaining']) <= 60
        assert float(entry['stopDeadline']) > time.time()
        statepath = root / 'wire_live.json'
        verify = args[-1] == '--verify-stopped'
        if mode.get('badStop'):
            print('stdout-only stopped claim: not a structured proof')
        elif verify and statepath.exists():
            print('QUANTNIGHT_STOP_OBSERVED ' + json.dumps({
                'schema_version': 1, 'task_name': name, 'target_remote': remote == '--remote',
                'session_absent': False}))
        else:
            if not verify and statepath.exists(): statepath.unlink()
            print('QUANTNIGHT_PAUSE_CONFIRMED ' + json.dumps({
                'schema_version': 2, 'task_name': name, 'target_remote': remote == '--remote',
                'session_absent': True, 'runners_exited': None, 'confirmation_basis': 'session_absent'}))
    elif args[0] in ('regular', 'super', 'priority'):
        kind, name, raw, host, profile = args[:5]
        config = json.loads(raw)
        if args[-1] == '--observe-accepted':
            assert kind == 'super' and config['accepted_observation']['remote'] == (host == '--remote')
            assert config['max_concurrent'] == config['max_multi_simulation_children'] == 1
            assert config['tag'] == {} and config['breakpoint'] is True
            print('SUPER_ACCEPTED_OBSERVATION ' + json.dumps({
                'binding': config['accepted_observation'], 'closed': True,
                'recovery': {'observed': 1, 'released': 0, 'unknown': mode.get('observationUnknown', 0)}}))
        else:
            assert len(args) == 5
            binding = json.loads(entry['binding'])
            assert binding['remote'] == (host == '--remote')
            if mode.get('launchLost'):
                print('launcher response lost after claim', file=sys.stderr)
                sys.exit(19)
            context = {'schemaVersion': 1, 'backend': 'tmux', 'binary': '/fixture/system/tmux',
                       'socketName': 'default', 'sessionId': '$fixture', 'sessionCreatedAt': 100,
                       'taskId': binding['taskId'], 'launchAttemptId': binding['launchAttemptId'],
                       'remote': binding['remote']}
            (root / 'wire_live.json').write_text(json.dumps({'name':name,'binding':binding,'config':config,'context':context}))
            print('QUANTNIGHT_TMUX_CONTEXT ' + json.dumps(context))
    elif args[:3] == ['run', '-m', 'backend.mongodb.mongo_datum']:
        name = args[args.index('--alpha_filename') + 1]
        assert '--template' in args and '--auth_profile' in args
        # Explicit fake generator effect; MemoryStore consumes these rows at its
        # storage boundary. No real generator or platform payload is evaluated.
        rows = [{'_id': {'$oid': '00000000000000000000000' + str(i)}, 'status': 'pending',
                 'regular': 'rank(close)', 'settings': {'region': 'USA'}, 'variantId': 'v' + str(i)}
                for i in (1, 2, 3)]
        (root / ('generated-' + name + '.json')).write_text(json.dumps(rows))
        print('fixture generator complete')
    else:
        assert args[0] == '-B'
        module = args[2] if args[1] == '-m' else args[1]
        rest = args[3:] if args[1] == '-m' else args[2:]
        flag = rest[0]
        if module == 'templates.catalog':
            assert flag == 'binding' and '--root' in rest and '--kind' in rest and '--name' in rest
            print(json.dumps({'ok': True, 'binding': {'recommendedMaxConcurrent': 3}}))
        else:
            request = json.loads(rest[1])
            command = flag.removeprefix('--')
            if command == 'validate-task-control-binding':
                assert request['root'] == str(root.resolve())
                assert request['missionDatabase'] == request['simulationDatabase'] == 'unused'
                assert 'localUriSha256' in request and 'remoteUriSha256' in request
                reply(command, None)
            elif module == 'scripts.script_config':
                assert flag == '--validate-execution-binding' and rest[2] == '--remote'
                assert request['remote'] == (rest[3] == 'true')
                reply(command, None) if mode.get('ownerFail') == command else print(json.dumps({
                    'schemaVersion': 1, 'command': command, 'ok': True, 'context': request.get('tmuxContext')}))
            elif module == 'scripts.priority_task_config':
                if flag == '--native-config-preflight':
                    exact(request, ['task', 'selected'])
                    config = {'interval': 600, 'refine_templates': []} if request['selected'] is None else dict(request['selected'])
                    config['auth_profile'] = request['task'].get('auth_profile', 'user1')
                    reply(command, config)
                elif flag == '--native-status-snapshot':
                    exact(request, ['task', 'remote', 'inspectItems', 'configurationBinding'])
                    reply(command, {'ok': not mode.get('priorityInvalid', False), 'remote': request['remote'],
                                    'inspected': request['inspectItems'], 'fixture': 'original-owner-boundary'})
                elif flag == '--native-default-config':
                    exact(request, ['task'])
                    reply(command, {'tag': {'tags': [request['task']['name']]}})
                else:
                    raise AssertionError(flag)
            elif module == 'util.super_capacity':
                exact(request, ['config'])
                assert flag == '--native-config-preflight'
                reply(command, {'validated': True})
            elif module == 'util.mission_recovery':
                exact(request, ['task', 'remote', 'configurationBinding'])
                reply(command, None)
            elif module == 'scripts.source_task_registration':
                exact(request, ['action', 'task', 'config'])
                assert request['action'] == 'bind-source'
                config = dict(request['config'])
                config.update(source_task_id=oid(request['task']['_id']), source_remote=request['task']['isRemote'])
                reply(command, config)
            elif module == 'util.dataset_mission':
                if flag == '--native-verify-registration':
                    exact(request, ['task', 'configurationBinding'])
                    reply(command, {'registeredCount': 3, 'simulationStarted': False,
                                    'receiptPath': str(root / 'registered.json'), 'payloadPath': str(root / 'payloads.json')})
                else:
                    assert flag == '--native-archive-statistics'
                    exact(request, ['task', 'remote', 'evidenceCollection', 'configurationBinding'])
                    assert request['remote'] == request['task']['isRemote']
                    name = request['task']['name']; tid = oid(request['task']['_id'])
                    assert request['evidenceCollection'] in (name, name + '__archive_' + tid)
                    reply(command, {'archivePath': str(root / 'research.json'), 'platformFetched': False})
            elif module.endswith('/watch_quant_task.py'):
                # The real owner's envelope is NOT the --native-control flag.
                command = 'native-mission-watcher'
                if request['action'] == 'preflight':
                    assert set(request) in ({'action', 'target', 'itemKey'}, {'action', 'task', 'target', 'itemKey'})
                    task = request.get('task')
                    result = {} if task is None else {'expectedBinding': {
                        'missionId': oid(task['_id']), 'launchId': task['launchAttemptId'], 'remote': task['isRemote']}}
                    reply(command, result)
                else:
                    exact(request, ['action', 'task', 'remote', 'spec', 'replace', 'configurationBinding'])
                    spec, task = request['spec'], request['task']
                    result = dict(spec, missionId=oid(task['_id']), launchId=task['launchAttemptId'],
                                  remote=request['remote'], producerPid=4242,
                                  producerMarkerPath=str(root / 'watch.lock'), checkpointPath=str(root / 'watch.json'),
                                  requiresLease='taskId' in spec['target'])
                    if mode.get('wrongWatch'): result['missionId'] = 'ffffffffffffffffffffffff'
                    reply(command, result)
            elif module == 'scripts.priority_cleanup':
                exact(request, ['task', 'remote', 'sharpe', 'fitness', 'dryRun', 'configurationBinding'])
                reply(command, {'dryRun': request['dryRun'], 'retired': 0 if request['dryRun'] else 2,
                                'keptStates': {'UNKNOWN': 1}})
            elif command == 'resolve-task-result-namespace':
                exact(request, ['authProfile', 'configurationBinding'])
                reply(command, {'database': 'fixture_results', 'collection': request['authProfile'] + '_alphas'})
            else:
                raise AssertionError((module, flag))
except Exception as exc:
    with (root / 'VIOLATIONS').open('a') as f:
        f.write(type(exc).__name__ + ': ' + str(exc) + '\n')
    raise
