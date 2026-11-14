#!/usr/bin/env python3
"""Synthetic TLS S3/KMS, independently generated V2/V3 envelopes and tamper checks."""
import argparse
import base64
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import shlex
import ssl
import subprocess
import tempfile
import threading
from compatibility import environment
from image_fixtures import png
from server_check import server, request

ENCRYPT = r'''
#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
int main(int n,char **v) {
 if(n!=5)return 1;
 unsigned char key[32],iv[12],in[65536],out[65552],tag[16];
 for(int i=0;i<32;i++){unsigned x;if(sscanf(v[1]+i*2,"%2x",&x)!=1)return 2;key[i]=(unsigned char)x;}
 for(int i=0;i<12;i++){unsigned x;if(sscanf(v[2]+i*2,"%2x",&x)!=1)return 2;iv[i]=(unsigned char)x;}
 FILE *f=fopen(v[3],"rb");if(!f)return 3;size_t len=fread(in,1,sizeof(in),f);fclose(f);
 EVP_CIPHER_CTX *c=EVP_CIPHER_CTX_new();int a,b;
 if(!c || !EVP_EncryptInit_ex(c,EVP_aes_256_gcm(),NULL,key,iv) || !EVP_EncryptUpdate(c,out,&a,in,(int)len)
 || !EVP_EncryptFinal_ex(c,out+a,&b) || !EVP_CIPHER_CTX_ctrl(c,EVP_CTRL_GCM_GET_TAG,16,tag))return 4;
 f=fopen(v[4],"wb");if(!f)return 5;
 if(fwrite(out,1,(size_t)(a+b),f)!=(size_t)(a+b)||fwrite(tag,1,16,f)!=16)return 6;
 fclose(f);EVP_CIPHER_CTX_free(c);return 0;
}
'''


def check(binary):
    binary = binary.resolve()
    if not json.loads(subprocess.check_output([str(binary),'capabilities'],env=environment())).get('s3_decryption'):
        print('S3 decryption is not compiled')
        return
    key, message_id = bytes(range(1,33)), bytes(range(28))
    prk = hmac.new(message_id, key, hashlib.sha512).digest()
    commit = hmac.new(prk,b'\x00\x73COMMITKEY\x01',hashlib.sha512).digest()[:28]
    derived = hmac.new(prk,b'\x00\x73DERIVEKEY\x01',hashlib.sha512).digest()[:32]
    errors, observed = [], []
    bad = ''
    with tempfile.TemporaryDirectory(prefix='pixstride-s3-crypto-') as temporary:
        root=Path(temporary)
        image=png(16,12)
        (root/'source.png').write_bytes(image)
        (root/'encrypt.c').write_text(ENCRYPT)
        flags=shlex.split(subprocess.check_output(['pkg-config','--cflags','--libs','openssl'],text=True))
        flags.extend('-Wl,-rpath,'+flag[2:] for flag in list(flags) if flag.startswith('-L'))
        subprocess.run(['cc',str(root/'encrypt.c'),'-o',str(root/'encrypt'),*flags],check=True,capture_output=True)
        cipher={}
        for version, encryption_key, nonce in ((2,key,bytes(range(12))),(3,derived,b'\x01'*12)):
            output=root/f'cipher{version}'
            subprocess.run([str(root/'encrypt'),encryption_key.hex(),nonce.hex(),str(root/'source.png'),str(output)],check=True,capture_output=True)
            cipher[version]=output.read_bytes()
        certificate=root/'certificate.pem'
        private=root/'tls-key.pem'
        (root/'openssl.cnf').write_text('[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=localhost\n[ext]\nsubjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,digitalSignature,keyEncipherment,keyCertSign\nextendedKeyUsage=serverAuth\n')
        openssl_prefix=subprocess.check_output(['pkg-config','--variable=prefix','openssl'],text=True).strip()
        openssl=str(Path(openssl_prefix)/'bin'/'openssl')
        subprocess.run([openssl,'req','-x509','-newkey','rsa:2048','-nodes','-days','1','-config',str(root/'openssl.cnf'),
                        '-keyout',str(private),'-out',str(certificate)],check=True,capture_output=True,timeout=20)

        class Peer(BaseHTTPRequestHandler):
            def do_POST(self):
                try:
                    assert self.path=='/kms'
                    body=self.rfile.read(int(self.headers['Content-Length']))
                    fields=json.loads(body)
                    algorithm=fields['EncryptionContext']['aws:x-amz-cek-alg']
                    assert algorithm in ('AES/GCM/NoPadding','115')
                    if bad=='mime':assert fields['EncryptionContext']['note']=='café'
                    assert base64.b64decode(fields['CiphertextBlob'])==b'synthetic-encrypted-key'
                    headers={k.lower():v for k,v in self.headers.items()}
                    match=re.fullmatch(r'AWS4-HMAC-SHA256 Credential=([^/]+)/([^,]+), SignedHeaders=([^,]+), Signature=([a-f0-9]{64})',headers['authorization'])
                    assert match
                    access,scope,signed,signature=match.groups()
                    assert access=='synthetic-cse-access'
                    date,region,service,terminator=scope.split('/')
                    assert region==('us-west-2' if fields['EncryptionContext']['fixture:bucket']=='other' else 'us-east-1') and service=='kms'
                    digest=hashlib.sha256(body).hexdigest()
                    canonical_headers=''.join(k+':'+' '.join(headers[k].split())+'\n' for k in signed.split(';'))
                    canonical='POST\n/kms\n\n'+canonical_headers+'\n'+signed+'\n'+digest
                    message='AWS4-HMAC-SHA256\n'+headers['x-amz-date']+'\n'+scope+'\n'+hashlib.sha256(canonical.encode()).hexdigest()
                    signing=b'AWS4synthetic-cse-secret'
                    for item in (date,region,service,terminator):signing=hmac.new(signing,item.encode(),hashlib.sha256).digest()
                    assert hmac.new(signing,message.encode(),hashlib.sha256).hexdigest()==signature
                    observed.append(algorithm)
                    result=json.dumps({'Plaintext':base64.b64encode(key if bad!='key' else b'x'*32).decode()}).encode()
                    self.send_response(200)
                except Exception as error:
                    errors.append(str(error));result=b'{}';self.send_response(400)
                self.send_header('Content-Length',str(len(result)));self.end_headers();self.wfile.write(result)

            def do_HEAD(self):
                self.image(False)

            def do_GET(self):
                self.image(True)

            def image(self, write):
                plain='plain.png' in self.path
                bucket='other' if '/other/' in self.path else 'images'
                assert ('/'+('us-west-2' if bucket=='other' else 'us-east-1')+'/s3/aws4_request') in self.headers['Authorization']
                version=3 if 'v3.png' in self.path else 2
                data=bytearray(image if plain else cipher[version])
                if bad=='tag':data[-1]^=1
                algorithm='115' if version==3 else 'AES/GCM/NoPadding'
                context=json.dumps({'aws:x-amz-cek-alg':'wrong' if bad=='context' else algorithm,'fixture:bucket':bucket})
                if bad=='mime':
                    raw=json.dumps({'aws:x-amz-cek-alg':algorithm,'fixture:bucket':bucket,'note':'café'},ensure_ascii=False).encode().decode('latin1').encode()
                    context='=?UTF-8?B?'+base64.b64encode(raw).decode()+'?='
                envelope=({'x-amz-3':base64.b64encode(b'synthetic-encrypted-key').decode(),'x-amz-i':base64.b64encode(message_id).decode(),
                           'x-amz-d':base64.b64encode(commit if bad!='commit' else b'x'*28).decode(),'x-amz-c':'115','x-amz-w':'12','x-amz-t':context}
                          if version==3 else {'x-amz-key-v2':base64.b64encode(b'synthetic-encrypted-key').decode(),
                           'x-amz-iv':base64.b64encode(bytes(range(12))).decode(),'x-amz-cek-alg':algorithm,'x-amz-wrap-alg':'kms+context',
                           'x-amz-matdesc':context,'x-amz-tag-len':'128'})
                self.send_response(206 if write else 200)
                self.send_header('Content-Length',str(len(data)));self.send_header('ETag','"synthetic-cipher"')
                if write:self.send_header('Content-Range',f'bytes 0-{len(data)-1}/{len(data)}')
                if not plain:
                    for name,value in envelope.items():self.send_header('x-amz-meta-'+name,value)
                self.end_headers()
                if write:self.wfile.write(data)

            def log_message(self,*_):pass

        origin=ThreadingHTTPServer(('127.0.0.1',0),Peer)
        tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(str(certificate),str(private))
        origin.socket=tls.wrap_socket(origin.socket,server_side=True)
        thread=threading.Thread(target=origin.serve_forever,daemon=True);thread.start()
        base=f'https://127.0.0.1:{origin.server_port}'
        try:
            settings={'PIXSTRIDE_USE_S3':'1','PIXSTRIDE_S3_ALLOWED_BUCKETS':'images,other','PIXSTRIDE_S3_REGION':'us-west-1',
                      'PIXSTRIDE_S3_BUCKET_REGIONS':'images=us-east-1,other=us-west-2',
                                    'PIXSTRIDE_ALLOWED_HOSTS':'127.0.0.1','PIXSTRIDE_ALLOW_LOOPBACK_SOURCE_ADDRESSES':'1',
                                    'PIXSTRIDE_ALLOWED_SOURCE_PORTS':str(origin.server_port),'PIXSTRIDE_S3_ENDPOINT':base+'/s3',
                                    'PIXSTRIDE_CLOUD_ROOT_CA_FILE':str(certificate),'PIXSTRIDE_S3_KMS_ENDPOINT':base+'/kms',
                                    'PIXSTRIDE_S3_ACCESS_KEY_ID':'synthetic-cse-access','PIXSTRIDE_S3_SECRET_ACCESS_KEY':'synthetic-cse-secret',
                                    'PIXSTRIDE_S3_USE_DECRYPTION_CLIENT':'1'}
            with server(binary,root,settings) as (port,_,_):
                for version in (2,3):
                    result=request(port,f'/unsafe/raw:1/plain/s3://images/v{version}.png@png')
                    assert result[0]==200 and result[2]==image, (result[:2],observed,errors,(root/'server.log').read_text())
                assert set(observed)=={'AES/GCM/NoPadding','115'} and not errors, errors
                assert request(port,'/unsafe/raw:1/plain/s3://other/v3.png@png')[2]==image
                bad='mime'
                assert request(port,'/unsafe/raw:1/plain/s3://images/v3.png@png')[2]==image
                for bad in ('tag','context','commit','key'):
                    result=request(port,'/unsafe/raw:1/plain/s3://images/v3.png@png')
                    assert result[0]==502 and all(value not in result[2] for value in (key,b'synthetic-cse-secret'))
                assert not errors,errors
                bad=''
                assert request(port,'/unsafe/raw:1/plain/s3://images/plain.png@png')[0]==502
            with server(binary,root,{**settings,'PIXSTRIDE_S3_USE_DECRYPTION_CLIENT':'0'}) as (port,_,_):
                result=request(port,'/unsafe/raw:1/plain/s3://images/plain.png@png')
                assert result[0]==200 and result[2]==image,result[:2]
        finally:
            origin.shutdown();origin.server_close();thread.join()
    print('TLS S3/KMS, SigV4, independently generated V2/V3, in-place AEAD and tamper rejection passed')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=Path,required=True)
    check(parser.parse_args().binary)
