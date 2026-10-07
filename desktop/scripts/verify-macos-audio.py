import argparse
import array
import ctypes
import json
import math
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import wave

root = pathlib.Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('library', nargs='?', type=pathlib.Path, default=root / 'desktop-resources/native/darwin-arm64/libmpv.2.dylib')
parser.add_argument('--native', action='store_true')
parser.add_argument('--ffmpeg', type=pathlib.Path)
arguments = parser.parse_args()
library = arguments.library.resolve()
lib = ctypes.CDLL(str(library))
lib.mpv_create.restype = ctypes.c_void_p
lib.mpv_set_option_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
lib.mpv_initialize.argtypes = [ctypes.c_void_p]
lib.mpv_request_log_messages.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
lib.mpv_command.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_char_p)]
lib.mpv_wait_event.argtypes = [ctypes.c_void_p, ctypes.c_double]
lib.mpv_wait_event.restype = ctypes.c_void_p
lib.mpv_get_property_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
lib.mpv_get_property_string.restype = ctypes.c_void_p
lib.mpv_free.argtypes = [ctypes.c_void_p]
lib.mpv_terminate_destroy.argtypes = [ctypes.c_void_p]


class Event(ctypes.Structure):
    _fields_ = [('id', ctypes.c_int), ('error', ctypes.c_int), ('userdata', ctypes.c_uint64), ('data', ctypes.c_void_p)]


class Log(ctypes.Structure):
    _fields_ = [('prefix', ctypes.c_char_p), ('level', ctypes.c_char_p), ('text', ctypes.c_char_p), ('log_level', ctypes.c_int)]


def property_value(handle, name):
    value = lib.mpv_get_property_string(handle, name.encode())
    if not value:
        return None
    try:
        return ctypes.string_at(value).decode()
    finally:
        lib.mpv_free(value)


def check(label, media):
    handle = lib.mpv_create()
    if not handle:
        raise RuntimeError('Could not create MPV')
    try:
        for key, value in {'vo': 'null', 'terminal': 'no', 'keep-open': 'yes', 'audio-device': 'auto', 'audio-channels': 'auto-safe', 'volume': '0'}.items():
            if lib.mpv_set_option_string(handle, key.encode(), value.encode()) < 0:
                raise RuntimeError(f'Could not set {key}')
        if lib.mpv_initialize(handle) < 0:
            raise RuntimeError('Could not initialize MPV')
        lib.mpv_request_log_messages(handle, b'v')
        command = (ctypes.c_char_p * 3)(b'loadfile', str(media).encode(), None)
        if lib.mpv_command(handle, command) < 0:
            raise RuntimeError('Could not load audio fixture')
        opened = []
        errors = []
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            event = Event.from_address(lib.mpv_wait_event(handle, 0.1))
            if event.id == 2 and event.data:
                log = Log.from_address(event.data)
                message = log.text.decode().strip()
                if 'AO: [' in message:
                    opened.append(message)
                if log.level in (b'error', b'fatal'):
                    errors.append(f'{log.prefix.decode()}: {message}')
        properties = {name: property_value(handle, name) for name in ['aid', 'current-ao', 'audio-out-params/format', 'audio-out-params/channels']}
        passed = bool(opened) and not errors and properties['current-ao'] == 'coreaudio' and bool(properties['audio-out-params/format']) and str(properties['aid']).isdigit()
        print(json.dumps({'fixture': label, 'passed': passed, 'opened': opened, 'errors': errors, 'properties': properties}), flush=True)
        return passed
    finally:
        lib.mpv_terminate_destroy(handle)


with tempfile.TemporaryDirectory(prefix='panorama-audio-check-') as directory:
    temporary = pathlib.Path(directory)
    fixtures = []
    for channels in (1, 2, 6):
        pcm = temporary / f'{channels}ch.wav'
        samples = array.array('h', (int(3000 * math.sin(2 * math.pi * (440 + channel * 110) * frame / 48000)) for frame in range(48000) for channel in range(channels)))
        with wave.open(str(pcm), 'wb') as output:
            output.setnchannels(channels)
            output.setsampwidth(2)
            output.setframerate(48000)
            output.writeframes(samples.tobytes())
        if channels == 2:
            fixtures.append(('PCM stereo', pcm))
        aac = temporary / f'{channels}ch.m4a'
        options = ['-l', 'AAC_5_1'] if channels == 6 else []
        subprocess.run(['/usr/bin/afconvert', '-f', 'm4af', '-d', 'aac', *options, str(pcm), str(aac)], check=True)
        fixtures.append((f'AAC {channels}ch', aac))
        if arguments.ffmpeg and channels in (2, 6):
            for codec in ('ac3', 'dca'):
                media = temporary / f'{codec}-{channels}ch.mkv'
                subprocess.run([str(arguments.ffmpeg), '-v', 'error', '-i', str(pcm), '-c:a', codec, '-strict', '-2', str(media)], check=True)
                fixtures.append((f'{codec.upper()} {channels}ch', media))
    results = [check(label, media) for label, media in fixtures]
    if arguments.native:
        environment = dict(os.environ)
        environment.pop('ELECTRON_RUN_AS_NODE', None)
        environment['MVK_CONFIG_LOG_LEVEL'] = '0'
        native = subprocess.run([str(root / 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'),
                                 str(root / 'desktop/scripts/verify-macos-native-audio.mjs'),
                                 str(library.parent / 'mpv_host.node'), *(str(media) for _, media in fixtures)], env=environment)
        results.append(native.returncode == 0)
    sys.exit(0 if all(results) else 1)
