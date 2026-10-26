"""Lowest process identity fixture for original Core25 checker/stopper.
No real psutil import or OS process scanning. Identity comes from the independently
recorded fake new-session/consumer publication, never today's caller binding.
"""
import json
from pathlib import Path
ROOT = Path(__file__).resolve().parent
STATUS_ZOMBIE = 'zombie'
class Error(Exception): pass
class NoSuchProcess(Error): pass

def state():
    p=ROOT/'os_state.json'
    return json.loads(p.read_text()) if p.exists() else None

def fault():
    p=ROOT/'os_fault.json'
    return json.loads(p.read_text()) if p.exists() else {}

def boot_time(): return 1500000000.0

class Process:
    def __init__(self,pid):
        self.pid=pid
        s=state();f=fault()
        if s is None or pid not in (1001,1002) or f.get('dead') or (pid==1002 and f.get('shell')):
            raise NoSuchProcess(pid)
    def create_time(self):
        s=state()
        if s is None: raise NoSuchProcess(self.pid)
        value=float(s['created']+(10 if self.pid==1002 else 0))
        return value+1 if fault().get('wrongProcessBirth') and self.pid==1002 else value
    def cmdline(self):
        s=state()
        if s is None: raise NoSuchProcess(self.pid)
        if self.pid==1001:return ['bash']
        argv=list(s['argv'])
        if fault().get('foreign'):
            argv[argv.index('--task')+1]='foreign'
        return argv
    def cwd(self): return str(ROOT)
    def is_running(self):
        f=fault()
        return state() is not None and not f.get('dead') and not (self.pid==1002 and f.get('shell'))
    def status(self):return 'running' if self.is_running() else STATUS_ZOMBIE

def process_iter(*args,**kwargs):
    with (ROOT/'VIOLATIONS').open('a') as f:f.write('unexpected whole process inventory fixture request\n')
    raise AssertionError('this selected-process fixture does not invent a full OS scan')
