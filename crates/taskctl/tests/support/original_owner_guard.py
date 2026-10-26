"""Execute ORIGINAL owners/embedded shell programs with lowest I/O fakes only.

Used solely in a disposable fixture root. No network, real Mongo or process scan.
The real original shell/checker/pause source is not rewritten except the existing
absolute-resolver fixture seam in script_config.py. Grace periods are unchanged.
"""
import json
import os
from pathlib import Path
import runpy
import socket
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
os.chdir(ROOT)
sys.path.insert(0, str(ROOT))
ORIGINAL = sys.argv[1:]
with (ROOT / 'owner_trace.jsonl').open('a') as f:
    f.write(json.dumps({'argv': ORIGINAL}) + '\n')

def forbidden(label):
    def fail(*args, **kwargs):
        with (ROOT / 'VIOLATIONS').open('a') as f:
            f.write(label + '\n')
        raise AssertionError(label)
    return fail

socket.socket.connect = forbidden('Python socket connect forbidden')
socket.socket.connect_ex = forbidden('Python socket connect_ex forbidden')
socket.getaddrinfo = forbidden('Python DNS forbidden')
import psutil
assert Path(psutil.__file__).resolve() == ROOT / 'psutil.py', 'must use lowest fake process identity'
import pymongo
import motor.motor_asyncio
pymongo.MongoClient.__init__ = forbidden('real MongoClient forbidden')
motor.motor_asyncio.AsyncIOMotorClient.__init__ = forbidden('real Motor client forbidden')

# Original source-registration main stays intact, including YAML/role selection,
# original read_source_task and inject_source_identity; only client storage is fake.
from backend.mongodb import mongo_config
from bson import json_util
class Cursor(list):
    def limit(self, n):
        assert n == 2
        return Cursor(self[:n])
class Collection:
    def find(self, query, projection):
        assert set(query) == {'name', 'status'} and query['status'] == {'$ne': 'deactive'}
        assert projection == {'_id': 1, 'name': 1, 'auth_profile': 1, 'taskType': 1, 'status': 1, 'isRemote': 1}
        # Mongo applies this metadata projection before returning typed rows.
        # Decode ONLY projected BSON metadata, never opaque saved business JSON
        # ($oid-like extension keys in startConfig are not BSON wrappers).
        rows = json.loads((ROOT / 'storage_snapshot.json').read_text())
        rows = [row for row in rows if row['name'] == query['name'] and row.get('status') != 'deactive']
        return Cursor(json_util.loads(json.dumps({k: v for k, v in row.items() if projection.get(k)})) for row in rows)
class Client:
    def __enter__(self): return self
    def __exit__(self, *args): pass
    def __getitem__(self, key):
        assert key == 'simulation_mission'
        return {'tasks': Collection()}
def fake_client(self, **kwargs):
    assert kwargs['target'] == self.resolve_target('local')
    assert kwargs['manage_atlas_access'] is False
    assert kwargs['serverSelectionTimeoutMS'] == kwargs['connectTimeoutMS'] == kwargs['socketTimeoutMS'] == 8000
    return Client()
mongo_config.MongoConfig.get_sync_client = fake_client

original_popen = subprocess.Popen
owned = set()
class GuardedPopen(original_popen):
    def __init__(self, args, *rest, **kwargs):
        assert not kwargs.get('shell'), 'shell=True is not a test seam'
        command = str(args[0])
        allowed = {str(ROOT / 'native-tmux'), 'ps', 'ssh', 'uv'}
        if command == 'bash':
            # Actual checker/stopper configuration reads only. Remote transport
            # execution is handled by the fake SSH leaf, not a second launcher.
            valid = (len(args) >= 5 and args[1] == '-c' and args[3] == 'config'
                     and str(ROOT / 'scripts') == str(args[4])
                     and args[2] in ('source "$1/script_config.sh"; get_remote_host',
                                     'source "$1/script_config.sh"; get_remote_workdir',
                                     'set -e; source "$1/script_config.sh"; "$2"'))
            if not valid: forbidden('unexpected original bash child')()
        elif command not in allowed:
            forbidden('unexpected process executable: ' + command)()
        super().__init__(args, *rest, **kwargs)
        owned.add(self.pid)
subprocess.Popen = GuardedPopen
original_kill = os.kill
def private_kill(pid, sig):
    if pid not in owned: forbidden('signal to unowned PID')()
    return original_kill(pid, sig)
os.kill = private_kill
os.killpg = forbidden('process group signaling forbidden')
original_execve = os.execve
def native_exec(path, args, env):
    if str(path) != str(ROOT / 'native-tmux'): forbidden('unexpected execve path')()
    return original_execve(path, args, env)
os.execve = native_exec

args = list(ORIGINAL)
if args[:1] == ['--uv']:
    args.pop(0)
    assert args.pop(0) == 'run'
    if args[:1] == ['--no-sync']: args.pop(0)
    if args[:1] == ['python']: args.pop(0)
while args[:1] in (['-B'], ['-u']): args.pop(0)
try:
    if args[:1] == ['-m']:
        module = args[1]
        if module in ('backend.mongodb.mongo_config', 'scripts.script_config', 'scripts.source_task_registration'):
            sys.argv = args[1:]
            runpy.run_module(module, run_name='__main__')
        elif module == 'util.mission_recovery':
            # This non-shell preflight remains stage-1 U wire. Do not claim the
            # full recovery owner is under test merely because shell is real.
            sys.argv = [str(ROOT / 'wire.py'), *ORIGINAL]
            runpy.run_path(str(ROOT / 'wire.py'), run_name='__main__')
        else:
            forbidden('unexpected original module: ' + module)()
    elif args[:1] == ['-c']:
        code = args[1]
        sys.argv = ['-c', *args[2:]]
        exec(compile(code, '<original-shell-embedded>', 'exec'), {'__name__': '__main__'})
    elif args[:1] == ['-']:
        sys.argv = ['-', *args[1:]]
        exec(compile(sys.stdin.read(), '<original-shell-stdin>', 'exec'), {'__name__': '__main__'})
    else:
        path = Path(args[0]).resolve()
        if path != ROOT / 'scripts/script_config.py': forbidden('unexpected Python script')()
        sys.argv = args
        runpy.run_path(str(path), run_name='__main__')
except SystemExit:
    if (ROOT / 'VIOLATIONS').exists(): raise SystemExit(96)
    raise
if (ROOT / 'VIOLATIONS').exists(): raise SystemExit(96)
