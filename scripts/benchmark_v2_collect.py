"""Collect all three engines, preserving service failures instead of dropping trials."""
import contextlib
import http.client
import io
import signal
import subprocess
import time

from benchmark_fixtures import sha256
from benchmark_metrics import balanced_orders,pixel_agreement,read_counters,summarize_batches
from benchmark_runtime import ENGINES,Server,counter_delta,request,signed_path

MIMES={'jpg':'image/jpeg','png':'image/png','webp':'image/webp','bmp':'image/bmp','avif':'image/avif'}
FORMATS={'jpg':'JPEG','png':'PNG','webp':'WEBP','bmp':'BMP','avif':'AVIF'}
EXPECTED_ERRORS=(ValueError,RuntimeError,OSError,http.client.HTTPException,subprocess.SubprocessError)


class DeadlineExpired(Exception):pass


@contextlib.contextmanager
def deadline(seconds):
    if signal.getitimer(signal.ITIMER_REAL)[0]:raise RuntimeError('A case deadline is already armed')
    previous=signal.getsignal(signal.SIGALRM)
    def expired(*_):raise DeadlineExpired('V2 case exceeded its declared wall-clock budget')
    signal.signal(signal.SIGALRM,expired);signal.setitimer(signal.ITIMER_REAL,seconds)
    try:yield
    finally:
        signal.setitimer(signal.ITIMER_REAL,0);signal.signal(signal.SIGALRM,previous)


def validate_response(case,status,headers,body,expected_hash=None):
    if status!=200:raise ValueError(f'Expected HTTP 200, received {status}: '+body[:160].decode(errors='replace'))
    mime=headers.get('content-type','').split(';')[0].strip().lower()
    if mime!=MIMES[case['format']]:raise ValueError(f'V2 output MIME: expected {MIMES[case["format"]]}, got {mime}')
    digest=sha256(body)
    if expected_hash is not None:
        if digest!=expected_hash:raise ValueError('A timed V2 response differs from decoded preflight')
    else:
        from PIL import Image
        with Image.open(io.BytesIO(body)) as image:
            image.load()
            if list(image.size)!=case['dimensions']:
                raise ValueError(f'V2 output dimensions: expected {case["dimensions"]}, got {list(image.size)}')
            if image.format!=FORMATS[case['format']] or getattr(image,'n_frames',1)!=1:
                raise ValueError('V2 output encoded format/static-frame contract differs')
    return dict(bytes=len(body),sha256=digest,mime=mime,dimensions=case['dimensions'])


def fail(entry,error,phase,batch=None):
    entry['status']='error'
    entry['error']=dict(type=type(error).__name__,message=str(error)[:2000],phase=phase,batch=batch)


def preflight_order(case):
    offset=(case.get('rank',1)-1)%len(ENGINES)
    return ENGINES[offset:]+ENGINES[:offset]


def remember_incomplete(result,latencies,protocol):
    active=result.get('active',{})
    if active.get('phase')!='measure':return
    engine,batch=active['engine'],active['batch'];entry=result['engines'][engine]
    if len(entry['batches'])==batch:
        position=balanced_orders(ENGINES,protocol['batches'])[batch].index(engine)
        entry['incomplete_batch']=dict(index=batch,position=position,latencies_ns=latencies.copy())


def collect(case,docker,images,fixtures,cpus,owner,index,protocol,mode,progress,flush):
    result=progress
    result.update({**case,'status':'running','engines':{
        e:dict(status='not-attempted',attempted=False,batches=[]) for e in ENGINES}})
    bodies={};servers={};path=signed_path(case);latencies=[];case_started=time.monotonic()
    result['preflight_order']=list(preflight_order(case))
    def active(engine,phase,batch=None):
        result['active']=dict(engine=engine,phase=phase,batch=batch);flush()
    try:
        # Cancel the work alarm before ownership-checked container cleanup starts.
        with contextlib.ExitStack() as stack,deadline(protocol['case_deadline_seconds']):
            # Rotate first-touch order; still try the reference after a Pixstride refusal.
            for engine in preflight_order(case):
                entry=result['engines'][engine];entry['attempted']=True;active(engine,'preflight')
                try:
                    image=images['imgproxy' if engine=='imgproxy' else 'pixstride']['reference']
                    server=stack.enter_context(Server(docker,engine,image,fixtures,cpus,owner,index));servers[engine]=server
                    status,headers,body,_=request(server.port,path)
                    entry['output']=validate_response(case,status,headers,body);bodies[engine]=body
                    entry['status']='ready'
                    if mode=='measure':
                        active(engine,'warmup')
                        for _ in range(protocol['warmups']):
                            status,headers,body,_=request(server.port,path)
                            validate_response(case,status,headers,body,entry['output']['sha256'])
                except EXPECTED_ERRORS as error:fail(entry,error,result['active']['phase'])
                flush()
            if mode=='measure':
                time.sleep(protocol['cpu_tail_seconds'])
                for batch,order in enumerate(balanced_orders(ENGINES,protocol['batches'])):
                    for position,engine in enumerate(order):
                        entry=result['engines'][engine]
                        if entry['status']!='ready':continue
                        latencies=[];active(engine,'measure',batch);server=servers[engine]
                        try:
                            before=read_counters(server.group);started=time.perf_counter_ns()
                            for _ in range(protocol['requests_per_batch']):
                                status,headers,body,elapsed=request(server.port,path)
                                validate_response(case,status,headers,body,entry['output']['sha256'])
                                latencies.append(elapsed)
                            time.sleep(protocol['cpu_tail_seconds']);after=read_counters(server.group)
                            counters=counter_delta(before,after)
                            counters.update(index=batch,position=position,latencies_ns=latencies,
                                            wall_elapsed_ns=time.perf_counter_ns()-started)
                            entry['batches'].append(counters)
                        except EXPECTED_ERRORS as error:
                            entry['incomplete_batch']=dict(index=batch,position=position,latencies_ns=latencies)
                            fail(entry,error,'measure',batch)
                        flush()
            for engine,server in servers.items():
                entry=result['engines'][engine];active(engine,'accounting')
                try:
                    entry['final_counters']=read_counters(server.group)
                    if entry['final_counters']['oom_kill']:raise ValueError('OOM invalidates the engine trial')
                    if entry['status']=='ready':
                        if mode=='measure':
                            entry['summary']=summarize_batches(entry['batches'])
                            entry['summary']['cgroup_peak_memory_bytes']=entry['final_counters']['memory_peak_bytes']
                        memory=dict(line.split() for line in (server.group/'memory.stat').read_text().splitlines())
                        entry['memory_at_end']={key:int(memory.get(key,0)) for key in ('anon','file','kernel')}
                        entry['status']='success'
                except EXPECTED_ERRORS as error:
                    if entry['status']!='error':fail(entry,error,'accounting')
                    else:entry['accounting_error']=str(error)[:1000]
                flush()
            if result['engines']['imgproxy']['status']=='success':
                for engine in ENGINES[:-1]:
                    entry=result['engines'][engine]
                    if entry['status']=='success':
                        active(engine,'pixels')
                        try:
                            entry['pixels_vs_imgproxy']=pixel_agreement(
                                bodies[engine],bodies['imgproxy'],
                                explicit_bmp_alpha=protocol.get('bmp_alpha')=='explicit-v5-masks')
                        except EXPECTED_ERRORS as error:fail(entry,error,'pixels')
                        flush()
            result['active']=dict(engine=None,phase='cleanup',batch=None);flush()
        result['status']='complete'
    except DeadlineExpired as error:
        remember_incomplete(result,latencies,protocol)
        for engine,entry in result['engines'].items():
            missing_pixels=(engine in ENGINES[:-1] and entry['status']=='success'
                            and 'pixels_vs_imgproxy' not in entry)
            if entry['status'] not in ('success','error') or missing_pixels:fail(entry,error,'deadline')
        result['status']='complete';result['deadline_exceeded']=True
    except BaseException:
        remember_incomplete(result,latencies,protocol)
        result['status']='interrupted'
        raise
    finally:
        if time.monotonic()-case_started>=protocol['case_deadline_seconds']:
            result['deadline_exceeded']=True
        result.pop('active',None);flush()
    return result
