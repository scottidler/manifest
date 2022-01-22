#!/usr/bin/env python3

import os
import re
import sys
sys.dont_write_bytecode = True

DIR = os.path.abspath(os.path.dirname(__file__))
CWD = os.path.abspath(os.getcwd())
REL = os.path.relpath(DIR, CWD)

REAL_FILE = os.path.abspath(__file__)
REAL_NAME = os.path.basename(REAL_FILE)
REAL_PATH = os.path.dirname(REAL_FILE)
if os.path.islink(__file__):
    LINK_FILE = REAL_FILE; REAL_FILE = os.path.abspath(os.readlink(__file__))
    LINK_NAME = REAL_NAME; REAL_NAME = os.path.basename(REAL_FILE)
    LINK_PATH = REAL_PATH; REAL_PATH = os.path.dirname(REAL_FILE)

from contextlib import contextmanager
from subprocess import Popen, PIPE, CalledProcessError
from argparse import ArgumentParser
from leatherman.dbg import dbg

ACTIONS =[
    'e', 'encrypt',
    'd', 'decrypt',
]

@contextmanager
def cd(*args, **kwargs):
    mkdir = kwargs.pop('mkdir', True)
    verbose = kwargs.pop('verbose', False)
    path = os.path.sep.join(args)
    path = os.path.normpath(path)
    path = os.path.expanduser(path)
    prev = os.getcwd()
    if path != prev:
        if mkdir:
            os.system(f'mkdir -p {path}')
        os.chdir(path)
        curr = os.getcwd()
        sys.path.append(curr)
        if verbose:
            print(f'cd {curr}')
    try:
        yield
    finally:
        if path != prev:
            sys.path.remove(curr)
            os.chdir(prev)
            if verbose:
                print('cd {prev}')

def call(cmd, stdout=PIPE, stderr=PIPE, shell=True, nerf=False, throw=True, verbose=False):
    if verbose or nerf:
        print(cmd)
    if nerf:
        return (None, 'nerfed', 'nerfed')
    process = Popen(cmd, stdout=stdout, stderr=stderr, shell=shell)
    stdout, stderr = [stream.decode('utf-8') for stream in process.communicate()]
    exitcode = process.poll()
    if verbose:
        if stdout:
            print(stdout)
        if stderr:
            print(stderr)
    if throw and exitcode:
        message = f'cmd={cmd}; stdout={stdout}; stderr={stderr}'
        raise CalledProcessError(exitcode, message)
    return exitcode, stdout, stderr


def encrypt(path, password, **kwargs):
    _, dirnames, filenames = next(os.walk(path))
    empty = True
    if filenames:
        location = os.path.dirname(path)
        targets = ' '.join(filenames)
        archive = f'{path}.7z'
        if os.path.isfile(archive):
            call('rm -rf {archive}')
        with cd(path):
            print(os.getcwd())
            call(f'7z -mhe=on -mhc=on a -p"{password}" "{archive}" {targets}', verbose=True)
            if os.path.isfile(archive):
                call(f'rmrf {targets}', verbose=True)
        empty = False
    if dirnames:
        for dirname in dirnames:
            path1 = os.path.join(path, dirname)
            empty1 = encrypt(path1, password, **kwargs)
    return empty

def decrypt(path, password, **kwargs):
    _, dirnames, filenames = next(os.walk(path))
    empty = True
    if filenames:
        location = os.path.dirname(path)
        archives = [filename for filename in filenames if filename.endswith('.7z')]
        with cd(path):
            for archive in archives:
                print(os.getcwd())
                dirname = os.path.splitext(archive)[0]
                call(f'mkdir -p {dirname}')
                call(f'7z e -p"{password}" -o{dirname} {archive}', verbose=True)
                call(f'rmrf {archive}', verbose=True)
    if dirnames:
        for dirname in dirnames:
            path1 = os.path.join(path, dirname)
            empty1 = decrypt(path1, password, **kwargs)
    return empty

def treecrypt(action, path, password, **kwargs):
    dict(
        e=encrypt, encrypt=encrypt,
        d=decrypt, decrypt=decrypt,
    )[action](
        os.path.abspath(path),
        password or input('password: ')
    )

def main(args):
    parser = ArgumentParser()
    parser.add_argument(
        '-p', '--password',
        default=os.environ.get('TREECRYPT_PASSWORD'),
        help='password to use to encrypt and decrypt 7z archives')
    parser.add_argument(
        'action',
        choices=ACTIONS,
        help='choose action to peform')
    parser.add_argument(
        'path',
        nargs='?',
        default=os.getcwd(),
        help='path to start recursively encrypt|decrypt')
    ns = parser.parse_args()
    treecrypt(**ns.__dict__)

if __name__ == '__main__':
    main(sys.argv[1:])

