"""Force the child's early exit to precede the harness's stdin write."""
from argparse import Namespace
from pathlib import Path
import errno
import subprocess
import sys

from smoke_stack import Smoke

run,binary=map(Path,sys.argv[1:])
binary=binary.resolve()
rig=Smoke(Namespace(run=run,slice='lifecycle',base_bin=None,head_bin=binary,
                    old_bin=None,middle_bin=None,source_dir=None))
for label,pin in [('valid','linked'),('bad-toml','malformed')]:
    app,_,_=rig.clone(label,pin)
    link=app/'meada.decisions.toml'
    before=(link.readlink(),link.read_bytes(),link.lstat().st_ino)
    for args in ((),('--item','no-such-item'),('--decider',),('--prune',)):
        argv=[binary,'decide',*args]
        with subprocess.Popen(argv,cwd=app,env=rig.env,stdin=subprocess.PIPE,
                              stdout=subprocess.PIPE,stderr=subprocess.PIPE) as child:
            assert child.wait(timeout=8)==1
            try:
                child.stdin.write(b'1\n')
                child.stdin.flush()
            except OSError as error:
                assert error.errno==errno.EPIPE, error
                print(label,args,'EPIPE from write after CLI exit')
            else:
                raise AssertionError((label,args,'pipe remained writable'))
            finally:
                try:
                    child.stdin.close()
                except BrokenPipeError:
                    pass
            expected=b'a symbolic link; Meada writes this file in the repository root only'
            assert child.stdout.read()==b'' and expected in child.stderr.read()
        output=subprocess.run(argv,cwd=app,env=rig.env,input=b'',capture_output=True,timeout=8)
        assert output.returncode==1 and output.stdout==b'' and expected in output.stderr
        assert (link.readlink(),link.read_bytes(),link.lstat().st_ino)==before
    print(label,'4/4 exact refusal diagnostics with empty stdin, link and target intact')
