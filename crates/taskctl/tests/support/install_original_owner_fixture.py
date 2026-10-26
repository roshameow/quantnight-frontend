"""Install selected original owners from an EXPLICIT backend source tree.
No live discovery, no dependency install; tmux resolver injection reuses the
original tests/tmux_context_fakes.py::install_support fixture.
"""
import hashlib
import importlib.util
import json
from pathlib import Path
import shlex
import shutil
import sys

root, source = map(lambda s: Path(s).resolve(), sys.argv[1:3])
assert root != source and root.is_dir() and source.is_dir()
local_uri, remote_uri = sys.argv[3:5]
files = ['scripts/script_config.py', 'scripts/script_config.sh', 'scripts/start_task.sh',
         'scripts/check_tmux_python.sh', 'scripts/pause_task.sh',
         'scripts/source_task_registration.py', 'backend/mongodb/mongo_config.py',
         '.agents/skills/manage-quantnight-frontend/scripts/taskctl.py']
hashes = {}
for relative in files:
    src = source / relative
    assert src.is_file() and not src.is_symlink()
    dest = root / relative
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dest)
    hashes[relative] = hashlib.sha256(src.read_bytes()).hexdigest()
for directory in ['scripts', 'backend', 'backend/mongodb']:
    (root / directory / '__init__.py').write_text('')
helper = source / 'tests/tmux_context_fakes.py'
assert helper.is_file()
spec = importlib.util.spec_from_file_location('original_tmux_leaf_fixtures', helper)
module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
resolver = 'NATIVE_TMUX_CANDIDATES = ("/opt/homebrew/bin/tmux", "/usr/local/bin/tmux", "/usr/bin/tmux", "/bin/tmux")'
assert (root / 'scripts/script_config.py').read_text().count(resolver) == 1, 'fixture resolver seam changed; do not run system tmux'
module.install_support(root / 'scripts', root / 'native-tmux')
modified = (root / 'scripts/script_config.py').read_text()
assert resolver not in modified
assert "NATIVE_TMUX_CANDIDATES = (" + repr(str(root / 'native-tmux')) + ',)' in modified
hashes['tests/tmux_context_fakes.py'] = hashlib.sha256(helper.read_bytes()).hexdigest()
(root / 'owner_source_hashes.json').write_text(json.dumps(hashes))
credentials = {'mongodb': {'local_uri': local_uri, 'remote_uri': remote_uri},
               'scripts': {'local_workdir': str(root), 'remote_workdir': str(root),
                           'remote_host': 'fixture-host', 'remote_env_path': str(root / 'activate'),
                           'tmp_dir': 'tmp', 'scripts': {'alpha_simulate': 'scripts.alpha_simulate_param',
                                                       'super_alpha_simulate': 'scripts.super_alpha_simulate_param'}}}
(root / 'credentials.yaml').write_text(json.dumps(credentials))
(root / 'activate').write_text('true\n')
python = shlex.quote(sys.executable)
guard = shlex.quote(str(root / 'original_owner_guard.py'))
leaf = shlex.quote(str(root / 'original_os_leaf.py'))
def executable(relative, body):
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('#!/bin/sh\n' + body + '\n'); path.chmod(0o700)
executable('.venv/bin/python', f'exec {python} {guard} "$@"')
executable('bin/python3', f'exec {python} {guard} "$@"')
executable('bin/uv', f'exec {python} {guard} --uv "$@"')
executable('native-tmux', f'exec {python} {leaf} tmux "$@"')
for name, mode in [('tmux', 'forbidden-tmux'), ('ps', 'ps'), ('ssh', 'ssh')]:
    executable('bin/' + name, f'exec {python} {leaf} {mode} "$@"')
(root / 'original_shell_enabled').write_text('absolute resolver verified; no real tmux/ps/ssh/runner\n')
print(json.dumps({'ok': True, 'source': str(source), 'files': hashes, 'resolver': str(root / 'native-tmux')}))
