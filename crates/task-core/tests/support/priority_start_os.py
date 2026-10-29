"""Install original Priority launcher/config owner; fake only OS process/SSH leaves.
Called by the native start route test, never a task launcher for real services.
"""
import json
import os
from pathlib import Path
import shlex
import shutil
import sys

root, backend = map(Path, sys.argv[1:3])
sys.path.insert(0, str(backend))
from scripts.priority_task_config import deployment_manifest
config = {"mission_tag": "pipeline", "refine_templates": [], "rounds": {"enabled": False}}
for relative in deployment_manifest(config, backend) + [Path("scripts/priority_task_config.py"), Path("scripts/start_priority_task.sh")]:
    if str(relative) in ("scripts/script_config.py", "scripts/script_config.sh"):
        continue  # existing fixture absolute tmux resolver and fake OS environment
    destination = root / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(backend / relative, destination)
credentials = root / "credentials.yaml"
credentials.write_text(credentials.read_text().replace("    super_alpha_simulate:", "    refine_simulate: scripts.refine_simulate_param\n    super_alpha_simulate:").replace("super_alpha_simulate: scripts.fake", "super_alpha_simulate: scripts.super_alpha_simulate_param").replace("alpha_simulate: scripts.fake", "alpha_simulate: scripts.alpha_simulate_param") + "\nmongodb:\n  local_uri: mongodb://offline.invalid\n  remote_uri: mongodb://offline.invalid\n")
# Real original preflight/configuration owners, with file/DB leaves below.
for package in ("api", "backend", "util"):
    for source in (backend / package).rglob("*.py"):
        if "__pycache__" in source.parts: continue
        destination=root/source.relative_to(backend);destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(source,destination)
for relative in ("api/api_config.yaml", "templates/catalog.py", "templates/__init__.py", "scripts/source_task_registration.py"):
    source=backend/relative
    if source.is_file():
        destination=root/relative;destination.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source,destination)
wrapper=root/".venv/bin/python"
source=wrapper.read_text()
begin=source.index('if [[ "${3:-}" == backend.mongodb.mongo_config ]]; then')
end=source.index('fi\n',begin)+3
wrapper.write_text(source[:begin]+source[end:])
# Original Priority shell does not source script_config before exec uv. Place
# its original uv executable boundary in the test PATH, with fixture-local env.
uv = root / "bin/uv"
uv.write_text("#!/bin/bash\nset -euo pipefail\n" +
    "export PATH=" + shlex.quote(str(root / "bin")) + ":/bin:/usr/bin\n" +
    "export STATE=" + shlex.quote(str(root / "state")) + " TRACE=" + shlex.quote(str(root / "trace")) +
    " PYTHONPATH=" + shlex.quote(str(root)) + " UV_OFFLINE=1\n" +
    "shift; [[ \"${1:-}\" == --no-sync ]] && shift\n" +
    "[[ \"${1:-}\" == python ]] && shift\n" +
    "exec " + shlex.quote(sys.executable) + " \"$@\"\n")
uv.chmod(0o755)
native = root / "native-tmux"
source = native.read_text().replace("a=a[2:]; state=Path(os.environ['STATE'])", """a=a[2:]; state=Path(os.environ['STATE'])
created=int(float((state.parent/'session-born').read_text())) if (state.parent/'session-born').exists() else 1700000000""")
source = source.replace("1700000000'", "'+str(created)+''")
source = source.replace("if a[0]=='new-session': state.write_text('active'); sys.exit(0)", """if a[0]=='new-session':
 import shlex
 assert not state.exists(), 'duplicate original launch'
 command=a[-1]
 if 'if uv run ' in command: script=command
 else:
  words=shlex.split(command);script=Path(words[1] if words[0]=='bash' else words[0]).read_text()
 line=next(line for line in script.splitlines() if 'if uv run ' in line)
 argv=shlex.split(line.split('if uv run ',1)[1].split('; then',1)[0])
 assert argv[0]=='-m' and argv[1] in ('scripts.refine_simulate_param','scripts.alpha_simulate_param','scripts.super_alpha_simulate_param'), argv
 index=argv.index('--config')+1
 if argv[index]=='$TMP_CONFIG_ABS':
  import re,base64
  raw=re.search(r'TMP_CONFIG_ABS=\$\(eval echo (.+)\)',script).group(1)
  argv[index]=shlex.split(raw)[0]
  encoded=re.search(r"echo '([^']+)' \| base64 -d",script).group(1)
  Path(argv[index]).write_bytes(base64.b64decode(encoded))
 assert Path(argv[index]).name=='tmp_config_fixture.json',argv
 (state.parent/'launched-argv.json').write_text(json.dumps(['python',*argv]))
 import time
 (state.parent/'session-born').write_text(str(time.time()))
 state.write_text('active')
 if (state.parent/'lose-launch-ack').exists(): sys.exit(23)
 sys.exit(0)""")
native.write_text(source)
(root / "psutil.py").write_text('''import os,json
from pathlib import Path
STATUS_ZOMBIE='zombie'
class NoSuchProcess(Exception): pass
class Error(Exception): pass
class Process:
 def __init__(self,pid):
  self.pid=pid
  if pid not in (1001,1002,1003,1005) or not Path(os.environ['STATE']).exists(): raise NoSuchProcess(pid)
 def create_time(self):
  import math
  root=Path(os.environ['STATE']).parent
  birth=float((root/'session-born').read_text())+{1001:0,1005:0.0001,1002:0.0002,1003:0.0003}[self.pid]
  if self.pid==1002 and (root/'nextafter-on-stop').exists() and 'executionObservation' in os.environ.get('QUANTNIGHT_TMUX_BINDING',''):
   return math.nextafter(birth,math.inf)
  return birth
 def cwd(self): return str(Path(os.environ['STATE']).parent)
 def cmdline(self):
  argv=json.loads((Path(os.environ['STATE']).parent/'launched-argv.json').read_text())
  return {1001:['bash','tmp/run_fixture.sh'],1005:['uv','run',*argv[1:]],1002:argv,1003:['python','-c','from multiprocessing.resource_tracker import main;main(17)']}[self.pid]
 def ppid(self): return {1001:1,1005:1001,1002:1005,1003:1002}[self.pid]
 def is_running(self): return Path(os.environ['STATE']).exists()
 def status(self): return 'running'
''')
(root / "bin/ps").write_text('''#!/bin/bash
if [[ "$*" == *lstart* ]]; then
 echo '9 1 Mon Jan 1 00:00:01 2024 S'
 if [[ -f "$STATE" ]]; then
  echo '1001 1 Mon Jan 1 00:00:01 2024 S'
  echo '1005 1001 Mon Jan 1 00:00:02 2024 S'
  echo '1002 1005 Mon Jan 1 00:00:03 2024 S'
  echo '1003 1002 Mon Jan 1 00:00:04 2024 S'
 fi
elif [[ -f "$STATE" ]]; then
 echo '1001 1 bash'; echo '1005 1001 uv'; echo '1002 1005 python'; echo '1003 1002 python'
fi
''')
with (root / "sitecustomize.py").open("a") as file:
    file.write("\nimport socket\ndef forbidden(*args,**kwargs): raise AssertionError('real socket forbidden in original Priority launcher fixture')\nsocket.socket.connect=forbidden\nsocket.create_connection=forbidden\n")

# Leaf Mongo replacement: original MongoConfig factory and source registration
# validator still run. Only the driver collection/cursor reads are fake.
with (root / "sitecustomize.py").open("a") as file:
    file.write("""
import pymongo
from pathlib import Path
class FixtureCursor(list):
 def limit(self,n): return FixtureCursor(self[:n])
class FixtureMongoClient:
 def __init__(self,*args,**kwargs):
  assert not args or str(args[0]).startswith('mongodb://offline.invalid'),args
 def __enter__(self): return self
 def __exit__(self,*args): pass
 def __getitem__(self,name):
  assert name in ('simulation_mission','tasks'),name
  return self
 def close(self): pass
 def find(self,query,projection):
  from bson import json_util
  row=json_util.loads(Path('original-control.json').read_text())
  assert query=={'name':row['name'],'status':{'$ne':'deactive'}},query
  assert projection.get('_id')==1
  return FixtureCursor([row])
pymongo.MongoClient=FixtureMongoClient
""")
