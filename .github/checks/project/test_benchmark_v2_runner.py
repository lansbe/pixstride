"""Actual HTTP/cgroup-file collection preserves losing engines and raw partial batches."""
import contextlib
import importlib.util
import io
import json
from http.server import BaseHTTPRequestHandler,HTTPServer
from pathlib import Path
import sys
import tempfile
import threading
import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/'scripts'))
import benchmark_v2_collect as collector
from benchmark_runtime import ENGINES,Server as RealServer
from benchmark_v2 import protocol,selected_cases
from benchmark_v2_catalog import catalog


@unittest.skipUnless(importlib.util.find_spec('PIL'),'Pillow installed by benchmark CI')
class RunnerV2Tests(unittest.TestCase):
    def test_preflight_start_order_rotates_without_preferential_reference_cache_warming(self):
        orders=[collector.preflight_order({'rank':rank}) for rank in (1,2,3)]
        self.assertEqual({order[0] for order in orders},set(ENGINES))
        self.assertTrue(all(set(order)==set(ENGINES) for order in orders))

    @contextlib.contextmanager
    def boundaries(self,failures=None,size=(16,16)):
        from PIL import Image
        stream=io.BytesIO();Image.new('RGB',size,'red').save(stream,format='PNG');body=stream.getvalue()
        servers={};threads=[];closed=[]
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            for engine in ENGINES:
                group=root/engine;group.mkdir()
                (group/'cpu.stat').write_text('usage_usec 1000\nuser_usec 800\nsystem_usec 200\n')
                (group/'memory.peak').write_text('1048576\n');(group/'memory.events').write_text('oom_kill 0\n')
                (group/'memory.stat').write_text('anon 1024\nfile 2048\nkernel 4096\n')
                def handler_for(name,directory):
                    class Handler(BaseHTTPRequestHandler):
                        calls=0
                        def do_GET(self):
                            type(self).calls+=1;calls=type(self).calls
                            usage=1000+calls*1000
                            (directory/'cpu.stat').write_text(f'usage_usec {usage}\nuser_usec {usage-200}\nsystem_usec 200\n')
                            bad=(failures or {}).get(name)
                            status=500 if bad and calls>=bad else 200
                            self.send_response(status);self.send_header('Content-Type','image/png')
                            self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
                        def log_message(self,*_):pass
                    return Handler
                server=HTTPServer(('127.0.0.1',0),handler_for(engine,group));servers[engine]=(server,group)
                thread=threading.Thread(target=lambda s=server:s.serve_forever(poll_interval=.01),daemon=True)
                thread.start();threads.append(thread)
            @contextlib.contextmanager
            def factory(docker,engine,*_):
                server,group=servers[engine]
                try:yield SimpleNamespace(port=server.server_port,group=group)
                finally:closed.append(engine)
            factory.bindings={e:SimpleNamespace(port=s.server_port,group=g) for e,(s,g) in servers.items()}
            try:yield factory,root,closed
            finally:
                for server,_ in servers.values():server.shutdown();server.server_close()
                for thread in threads:thread.join(timeout=2)

    def collect(self,mode='measure',failures=None):
        with self.boundaries(failures) as (factory,root,closed):
            case=dict(id='test',label='Test',source='input.png',options='',format='png',dimensions=[16,16])
            p={**protocol(mode),'requests_per_batch':1,'warmups':0,'cpu_tail_seconds':0}
            images={product:{'reference':'test'} for product in ('pixstride','imgproxy')};progress={};saved=[]
            with patch.object(collector,'Server',side_effect=factory):
                result=collector.collect(case,None,images,root,[0,1],'test',0,p,mode,progress,
                                         lambda:saved.append(json.loads(json.dumps(progress))))
            self.assertEqual(set(closed),set(ENGINES));self.assertTrue(saved)
            return result

    def test_preflight_failure_does_not_hide_other_engines_or_become_fast_win(self):
        result=self.collect(failures={'pixstride_default':1})
        self.assertEqual(result['status'],'complete')
        self.assertEqual(result['engines']['pixstride_default']['status'],'error')
        self.assertEqual(result['engines']['pixstride_default']['error']['phase'],'preflight')
        for engine in ('imgproxy','pixstride_native'):
            e=result['engines'][engine];self.assertEqual(e['status'],'success')
            self.assertEqual(len(e['batches']),6);self.assertGreater(e['summary']['cpu_ms_per_request'],0)
        self.assertTrue(result['engines']['pixstride_native']['pixels_vs_imgproxy']['exact_visible_pixels'])

    def test_mid_measurement_failure_retains_partial_batches_and_finishes_reference(self):
        result=self.collect(failures={'pixstride_native':4})
        e=result['engines']['pixstride_native'];self.assertEqual(e['status'],'error')
        self.assertEqual(len(e['batches']),2);self.assertEqual(e['error']['batch'],2)
        self.assertEqual(len(result['engines']['imgproxy']['batches']),6)
        self.assertNotIn('summary',e)

    def test_preflight_never_produces_qualified_timing_samples(self):
        result=self.collect(mode='preflight')
        for entry in result['engines'].values():
            self.assertEqual(entry['status'],'success');self.assertEqual(entry['batches'],[])
            self.assertNotIn('summary',entry)
        self.assertEqual(protocol('measure')['requests_per_batch'],24)
        self.assertEqual(protocol('preflight')['requests_per_batch'],0)
        with self.assertRaises(ValueError):protocol('fast-but-called-full')

    def test_five_output_codecs_are_independently_decoded_and_validated(self):
        from PIL import Image
        for fmt,pillow,mime in [('png','PNG','image/png'),('jpg','JPEG','image/jpeg'),('webp','WEBP','image/webp'),('bmp','BMP','image/bmp'),('avif','AVIF','image/avif')]:
            stream=io.BytesIO();Image.new('RGB',(16,16),'red').save(stream,format=pillow);body=stream.getvalue()
            case=dict(format=fmt,dimensions=[16,16]);meta=collector.validate_response(case,200,{'content-type':mime},body)
            self.assertEqual(meta['dimensions'],[16,16])
            for change in ('status','mime','dimension','hash'):
                with self.subTest(format=fmt,change=change),self.assertRaises(ValueError):
                    collector.validate_response({**case,'dimensions':[1,1]} if change=='dimension' else case,
                        500 if change=='status' else 200,{'content-type':'wrong' if change=='mime' else mime},
                        body,'a'*64 if change=='hash' else None)

    def test_deadline_is_real_and_releases_its_signal_handler(self):
        import signal
        old=signal.getsignal(signal.SIGALRM)
        with self.assertRaises(collector.DeadlineExpired):
            with collector.deadline(.01):time.sleep(.1)
        self.assertEqual(signal.getsignal(signal.SIGALRM),old)
        self.assertEqual(signal.getitimer(signal.ITIMER_REAL),(0.0,0.0))

    def test_interrupt_keeps_completed_batches_and_cleans_every_owned_server(self):
        original=collector.request;calls=0;progress={}
        def interrupted(*args):
            nonlocal calls
            calls+=1
            if calls==10:raise KeyboardInterrupt()
            return original(*args)
        with self.boundaries() as (factory,root,closed):
            case=dict(id='test',label='Test',source='input.png',options='',format='png',dimensions=[16,16])
            p={**protocol('measure'),'requests_per_batch':1,'warmups':0,'cpu_tail_seconds':0}
            images={product:{'reference':'test'} for product in ('pixstride','imgproxy')}
            with patch.object(collector,'Server',side_effect=factory),patch.object(collector,'request',side_effect=interrupted):
                with self.assertRaises(KeyboardInterrupt):
                    collector.collect(case,None,images,root,[0,1],'test',0,p,'measure',progress,lambda:None)
            self.assertEqual(set(closed),set(ENGINES))
            self.assertEqual(progress['status'],'interrupted')
            self.assertTrue(all(len(e['batches'])==2 for e in progress['engines'].values()))

    def test_case_deadline_preserves_already_accepted_samples_in_its_incomplete_batch(self):
        original=collector.request;calls=0;progress={}
        def expires(*args):
            nonlocal calls
            calls+=1
            if calls==6:raise collector.DeadlineExpired('test case budget')
            return original(*args)
        with self.boundaries() as (factory,root,closed):
            case=dict(id='test',label='Test',source='input.png',options='',format='png',dimensions=[16,16])
            p={**protocol('measure'),'requests_per_batch':3,'warmups':0,'cpu_tail_seconds':0}
            images={product:{'reference':'test'} for product in ('pixstride','imgproxy')}
            with patch.object(collector,'Server',side_effect=factory),patch.object(collector,'request',side_effect=expires):
                collector.collect(case,None,images,root,[0,1],'test',0,p,'measure',progress,lambda:None)
            self.assertEqual(set(closed),set(ENGINES));self.assertTrue(progress['deadline_exceeded'])
            own=progress['engines']['pixstride_default']
            self.assertEqual(own['status'],'error');self.assertIn('incomplete_batch',own)
            self.assertEqual(len(own['incomplete_batch']['latencies_ns']),2)

    def test_deadline_during_pixels_preserves_explicit_failures_that_still_aggregate(self):
        from benchmark_v2_report import aggregate
        from test_benchmark_v2_report import evidence
        case=next(c for c in catalog()['cases'] if c['format']=='png')
        self.assertLessEqual(case['rank'],25)
        with self.boundaries(size=tuple(case['dimensions'])) as (factory,root,closed):
            images={product:{'reference':'test'} for product in ('pixstride','imgproxy')};progress={}
            with patch.object(collector,'Server',side_effect=factory), \
                    patch.object(collector,'pixel_agreement',side_effect=collector.DeadlineExpired('pixel budget')):
                collector.collect(case,None,images,root,[0,1],'test',0,protocol('preflight'),'preflight',progress,lambda:None)
            self.assertEqual(set(closed),set(ENGINES));self.assertTrue(progress['deadline_exceeded'])
            for engine in ENGINES[:-1]:
                self.assertEqual(progress['engines'][engine]['status'],'error')
                self.assertEqual(progress['engines'][engine]['error']['phase'],'deadline')
            reports,pins=evidence();reports[0]['cases'][case['rank']-1]=progress
            result=aggregate(reports,'preflight',pins=pins)
            self.assertTrue(result['reference_valid']);self.assertIsNone(result['qualified'])
            self.assertEqual(result['architectures']['amd64']['by_mode']['pixstride_native']['http_valid_cases'],999)

    def test_case_alarm_cannot_interrupt_owned_cleanup_and_elapsed_budget_still_counts(self):
        import signal
        with self.boundaries() as (factory,root,_):
            class CleanupDocker:
                alive={'foreign-container':'foreign-owner'}
                created=[];removed=[];first=True;cleanup_done=False
                def inspect(self,name):
                    if name not in self.alive:raise RuntimeError('not found')
                    if self.first:
                        self.first=False
                        # Compress a pending real deadline into cleanup; a protected cleanup has no armed alarm.
                        if signal.getitimer(signal.ITIMER_REAL)[0]:signal.setitimer(signal.ITIMER_REAL,.01)
                        time.sleep(.03)
                    return {'Config':{'Labels':{'io.pixstride.benchmark.run':self.alive[name]}}}
                def run(self,*args,**_):
                    if args[0]=='logs':return ''
                    if args[:2]!=('rm','-f'):raise AssertionError(args)
                    name=args[-1]
                    if self.alive[name]!='test':raise AssertionError('foreign removal')
                    self.removed.append(name);del self.alive[name]
                    self.cleanup_done=len(self.removed)==3
                    return ''
            docker=CleanupDocker()
            class RunningServer(RealServer):
                def __enter__(self):
                    binding=factory.bindings[self.engine];self.port=binding.port;self.group=binding.group
                    self.docker.alive[self.name]=self.owner;self.docker.created.append(self.name)
                    return self
            case=dict(id='test',label='Test',source='input.png',options='',format='png',dimensions=[16,16])
            images={product:{'reference':'test'} for product in ('pixstride','imgproxy')};progress={}
            clock=time.monotonic
            with patch.object(collector,'Server',RunningServer), \
                    patch.object(collector.time,'monotonic',side_effect=lambda:clock()+(601 if docker.cleanup_done else 0)):
                collector.collect(case,docker,images,root,[0,1],'test',0,protocol('preflight'),'preflight',progress,lambda:None)
            self.assertEqual(docker.alive,{'foreign-container':'foreign-owner'})
            self.assertEqual(set(docker.removed),set(docker.created));self.assertEqual(len(docker.removed),3)
            self.assertTrue(progress['deadline_exceeded'])
            self.assertEqual(signal.getitimer(signal.ITIMER_REAL),(0.0,0.0))

    def test_selected_case_cannot_masquerade_as_complete_shard(self):
        cases,partial=selected_cases(0,None);self.assertEqual(len(cases),25);self.assertFalse(partial)
        chosen,partial=selected_cases(0,[cases[0]['id']]);self.assertEqual(len(chosen),1);self.assertTrue(partial)
        with self.assertRaises(ValueError):selected_cases(0,[catalog()['cases'][-1]['id']])
