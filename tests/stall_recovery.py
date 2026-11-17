"""End-to-end recovery check with fake CUPS; run after cargo build --locked."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

BINARY = Path(__file__).resolve().parents[1] / 'target/debug/paperjam'


def check(cancel_fails=False):
    with tempfile.TemporaryDirectory(prefix='paperjam-recovery-') as tmp:
        root = Path(tmp)
        (root / 'data/games').mkdir(parents=True)
        (root / 'data/pdfs').mkdir()
        (root / 'bin').mkdir()
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        (root / 'data/app.toml').write_text(f'bind = "127.0.0.1:{port}"\n')
        (root / 'data/printer.toml').write_text('backend="cups"\ndestination="test"\npoll_interval_ms=20\n[profiles.a]\nname="A"\n[profiles.b]\nname="B"\n')
        game = 'id="sample"\nname="Sample"\n[[episodes]]\nid="main"\nname="Main"\n[[documents]]\nid="white"\nname="White"\nfile="sample.pdf"\ndefault_profile="a"\n'
        for page in range(1, 4):
            game += f'[[documents.elements]]\nid="page-{page}"\nname="Page {page}"\npage={page}\nepisode="main"\nprofile="{"b" if page == 2 else "a"}"\n'
        (root / 'data/games/sample.toml').write_text(game)
        # Minimal three-page PDF, no external PDF tools needed.
        objects = ['<< /Type /Catalog /Pages 2 0 R >>', '<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>']
        objects += ['<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>'] * 3
        pdf = b'%PDF-1.4\n'
        offsets = [0]
        for n, obj in enumerate(objects, 1):
            offsets.append(len(pdf))
            pdf += f'{n} 0 obj\n{obj}\nendobj\n'.encode()
        xref = len(pdf)
        pdf += b'xref\n0 6\n0000000000 65535 f \n'
        pdf += b''.join(f'{o:010} 00000 n \n'.encode() for o in offsets[1:])
        pdf += f'trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
        (root / 'data/pdfs/sample.pdf').write_bytes(pdf)
        fake = f'''#!{os.sys.executable}
import sys,json
from pathlib import Path
root=Path({str(root)!r})
name=Path(sys.argv[0]).name
if name=='lp':
 p=root/'submissions'
 rows=json.loads(p.read_text()) if p.exists() else []
 rows.append(sys.argv[1:]); p.write_text(json.dumps(rows))
 n=len(rows)
 if n==2:(root/'queued').write_text('test-2 user 100')
 print(f'request id is test-{{n}} (1 file(s))')
elif name=='lpstat':
 p=root/'queued'
 print(p.read_text() if p.exists() else '')
else:
 (root/'cancelled').write_text(json.dumps(sys.argv[1:]))
 if {cancel_fails!r}:
  print('simulated cancellation failure',file=sys.stderr);sys.exit(1)
 (root/'queued').unlink(missing_ok=True)
'''
        for name in ('lp', 'lpstat', 'cancel'):
            path = root / 'bin' / name
            path.write_text(fake)
            path.chmod(0o755)
        env = {k: v for k, v in os.environ.items() if not k.startswith('PAPERJAM_')}
        env['PATH'] = str(root / 'bin') + os.pathsep + env['PATH']
        proc = subprocess.Popen([str(BINARY)], cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            def api(path, body=None):
                req = urllib.request.Request(f'http://127.0.0.1:{port}/api/{path}', data=json.dumps(body).encode() if body is not None else None, headers={'Content-Type': 'application/json'})
                with urllib.request.urlopen(req, timeout=3) as response:
                    return json.load(response)

            def wait(predicate):
                deadline = time.monotonic() + 8
                while time.monotonic() < deadline:
                    try:
                        result = predicate()
                        if result:
                            return result
                    except urllib.error.URLError:
                        pass
                    time.sleep(0.02)
                raise AssertionError('Timed out waiting for Paperjam')

            wait(lambda: api('catalog'))
            plan = api('plans/production', dict(game_id='sample', document_id='white', target_games=1, selected_element_ids=['page-1', 'page-2', 'page-3'], inventory_mode='ignore_inventory', group_by_profile=False))
            run = api('runs', {'plan_id': plan['id']})
            path = 'runs/' + run['id']
            wait(lambda: (r := api(path))['current_job'] == 1 and r['jobs'][1]['status'] == 'submitted')
            before = api('inventory')
            # Stale clicks must not recover the next batch.
            try:
                api(path + '/recover', {'cups_request_id': 'test-1'})
                raise AssertionError('Stale recovery accepted')
            except urllib.error.HTTPError as error:
                assert error.code == 400
            api(path + '/recover', {'cups_request_id': 'test-2'})
            result = wait(lambda: (r if (r := api(path))['status'] in ('completed', 'failed') else None))
            rows = json.loads((root / 'submissions').read_text())
            assert json.loads((root / 'cancelled').read_text()) == ['test-2']
            assert api('inventory') == before
            if cancel_fails:
                assert result['status'] == 'failed', result
                assert len(rows) == 2, rows
                assert 'simulated cancellation failure' in result['error']
            else:
                assert result['status'] == 'completed', result
                assert len(rows) == 4, rows
                assert rows[1] == rows[2], 'Retry changed original print settings'
                assert [row[row.index('-P') + 1] for row in rows] == ['1', '2', '2', '3']
        finally:
            proc.terminate()
            proc.wait(timeout=5)
            proc.stderr.close()


if __name__ == '__main__':
    check()
    check(cancel_fails=True)
    print('PASS: recovery retries only the stalled batch; cancellation failure prevents duplicates')
