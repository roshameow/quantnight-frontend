"""Leaf fake OS reused in shape from tmux_context_fakes/system_tmux tests.
Never invokes a system tmux/ps/SSH or the runner command in new-session.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import base64
import re
ROOT = Path(__file__).resolve().parent
kind, args = sys.argv[1], sys.argv[2:]
statefile = ROOT / 'os_state.json'
fault = json.loads((ROOT / 'os_fault.json').read_text()) if (ROOT / 'os_fault.json').exists() else {}
state = json.loads(statefile.read_text()) if statefile.exists() else None
with (ROOT / 'os_trace.jsonl').open('a') as f:
    f.write(json.dumps({'kind': kind, 'argv': args}) + '\n')
def violation(message):
    with (ROOT / 'VIOLATIONS').open('a') as f: f.write(message + '\n')
    raise AssertionError(message)
try:
    if kind == 'forbidden-tmux': violation('PATH tmux used instead of absolute native resolver')
    if kind == 'tmux':
        assert args[:2] == ['-L', 'default']
        assert not any(k in os.environ for k in ('TMUX', 'TMUX_PANE', 'TMUX_TMPDIR'))
        args = args[2:]; command = args[0]
        if fault.get('denied'):
            print('permission denied', file=sys.stderr); sys.exit(1)
        if fault.get('unknown'):
            print('unrecognized checker stdout'); sys.exit(0)
        if command == 'new-session':
            assert state is None and '-s' in args and '-d' in args
            name = args[args.index('-s') + 1]
            counterfile = ROOT / 'os_birth.txt'
            birth = int(counterfile.read_text()) + 1 if counterfile.exists() else 1700000000
            counterfile.write_text(str(birth))
            binding=json.loads(os.environ['QUANTNIGHT_TMUX_BINDING'])
            remote=binding['remote']
            path=ROOT/'tmp'/('tmp_config_'+name+'.json')
            if remote:
                script=Path(args[-1])
                assert script.is_relative_to(ROOT/'tmp')
                encoded=re.findall(r"echo '([^']+)' \| base64 -d",script.read_text())
                assert len(encoded)==1
                loaded=json.loads(base64.b64decode(encoded[0]))
            else:loaded=json.loads(path.read_text())
            # Actual original launcher uses a file --config. Do not invent an
            # inline binding or reopen mutable config as ownership evidence.
            argv=['python','-m','scripts.alpha_simulate_param','--task',name,
                  '--dist','remote' if remote else 'local','--auth_profile','user1','--config',str(path)]
            statefile.write_text(json.dumps({'name':name,'sid':'$4','created':birth,'argv':argv,'loaded':loaded}))
            assert loaded['source_task_id']==binding['taskId'] and loaded['source_remote'] is remote
            sys.path.insert(0,str(ROOT/'scripts'))
            from script_config import loaded_config_digest
            capability=dict(task=name,auth_profile='user1',source_task_id=loaded['source_task_id'],
                source_remote=remote,dist='remote' if remote else 'local',consumer_run_id='fixture-run-'+str(birth),
                pid=1002,pidCreatedAt=float(birth+10),configDigestVersion=1,
                effectiveConfigDigest=loaded_config_digest(loaded,name,remote,'user1'))
            (ROOT/'os_capability.json').write_text(json.dumps(capability))
            if fault.get('shell') or fault.get('dead'):
                next_fault={k:v for k,v in fault.items() if k not in ('shell','dead')}
                (ROOT/'os_fault.json').write_text(json.dumps(next_fault))
            # Do not execute the runner shell/sleep supplied by original launcher.
        elif command == 'list-sessions':
            if state is None:
                print('no server running on /fixture/default', file=sys.stderr); sys.exit(1)
            if '#{pid}' in args[-1]: print(f"{state['sid']}|{state['name']}|500|{state['created']}")
            else: print(f"{state['sid']}|{state['name']}")
        elif command == 'display-message':
            assert state is not None and args[-1] == '#{session_id}|#{session_name}|#{session_created}'
            print(f"{state['sid']}|{state['name']}|{state['created']}")
        elif command == 'list-panes':
            assert state is not None
            if args[-1] == '#{session_id}|#{pane_pid}': print(f"{state['sid']}|1001")
            elif args[-1] == '#{session_id}|#{pane_pid}|#{pane_dead}': print(f"{state['sid']}|1001|{1 if fault.get('dead') else 0}")
            else:
                assert args[-1] == '#{session_id}|#{window_index}|#{pane_index}|#{pane_pid}|#{pane_current_command}|#{pane_dead}'
                print(f"{state['sid']}|0|0|1001|bash|{1 if fault.get('dead') else 0}")
        elif command == 'if-shell':
            assert state is not None
            assert args[1:4] == ['-F', '-t', state['sid']]
            assert all(str(x) in args[4] for x in [state['sid'], state['name'], '500', state['created']])
            assert args[5] in ("send-keys -t '$4' C-c", "kill-session -t '$4'")
            statefile.unlink()
        else: violation('unexpected tmux command: ' + command)
    elif kind == 'ps':
        assert args[:1] == ['-axo']
        if args[1:] == ['pid=,ppid=,lstart=,stat=']:
            print('9 1 Mon Jan 1 00:00:01 2024 S')
            if state and not fault.get('dead'):
                print('1001 1 Mon Jan 1 00:00:01 2024 S')
                if not fault.get('shell'): print('1002 1001 Mon Jan 1 00:00:01 2024 S')
        elif args[1:] == ['pid=,ppid=,comm=']:
            print('9 1 fixture')
            if state and not fault.get('dead'):
                print('1001 1 bash')
                if not fault.get('shell'): print('1002 1001 python')
        else: violation('unexpected ps fields')
    elif kind == 'ssh':
        if fault.get('sshFail'): sys.exit(255)
        while args[:1] == ['-o']: args = args[2:]
        if args[:1] == ['--']: args.pop(0)
        assert args.pop(0) == 'fixture-host'
        # Replay only original supplied shell transport locally, with fixture
        # PATH and cwd; absolutely no SSH client or external connection exists.
        if args[:1] == ['bash']:
            assert args == ['bash', '-se']
            command = ['/bin/bash', '-se']
        else:
            assert len(args) == 1 and ('python3 -u -' in args[0] or 'uv run --no-sync python -' in args[0])
            command = ['/bin/bash', '-c', args[0]]
        sys.exit(subprocess.run(command, input=sys.stdin.buffer.read(), cwd=ROOT).returncode)
    else: violation('unexpected OS leaf: ' + kind)
except SystemExit:
    raise
except BaseException as exc:
    violation(type(exc).__name__ + ': ' + str(exc))
