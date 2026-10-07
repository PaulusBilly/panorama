import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tarfile

root = pathlib.Path(__file__).resolve().parents[2]
manifest_path = root / 'desktop/native/mpv-host/macos-libmpv.json'
manifest = json.loads(manifest_path.read_text())
cache = root / '.cache/panorama/macos-libmpv'
prefix = cache / 'current'
sources = cache / 'sources'
downloads = cache / 'downloads'
builds = cache / 'builds'
tools = root / '.cache/panorama/macos-build-tools'
if platform.system() != 'Darwin' or platform.machine() != 'arm64':
    sys.exit('The macOS native runtime requires an Apple Silicon Mac.')
for directory in (prefix, sources, downloads, builds):
    directory.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, PATH=f'{tools}/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin',
           PKG_CONFIG_LIBDIR=f'{prefix}/lib/pkgconfig', PKG_CONFIG_PATH='',
           CMAKE_PREFIX_PATH=str(prefix), MACOSX_DEPLOYMENT_TARGET=manifest['deploymentTarget'],
           CFLAGS=f'-I{prefix}/include', CXXFLAGS=f'-I{prefix}/include',
           LDFLAGS=f'-L{prefix}/lib')

def run(args, cwd=root):
    subprocess.run([str(arg) for arg in args], cwd=cwd, env=env, check=True)

if not (tools / 'bin/meson').exists():
    run([sys.executable, '-m', 'venv', tools])
    run([tools / 'bin/pip', 'install', 'meson==1.7.2', 'mako==1.3.10',
         'jinja2==3.1.6', 'packaging==24.2', 'MarkupSafe==3.0.3'])

paths = {}
for item in manifest['sources']:
    archive = downloads / item['archive']
    if not archive.exists():
        temporary = archive.with_suffix('.download')
        run(['curl', '--fail', '--location', '--retry', '2', item['url'], '-o', temporary])
        temporary.rename(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != item['sha256']:
        sys.exit(f"Checksum mismatch: {item['name']}")
    destination = sources / item['name']
    if not destination.exists():
        destination.mkdir()
        with tarfile.open(archive) as content:
            members = content.getmembers()
            for member in members:
                parts = pathlib.PurePosixPath(member.name).parts
                if '..' in parts or member.name.startswith('/'):
                    sys.exit('Unsafe source archive')
            content.extractall(destination, filter='data')
        for metadata in destination.glob('.DS_Store'):
            metadata.unlink()
        children = list(destination.iterdir())
        if len(children) == 1 and children[0].is_dir():
            extracted = destination.with_name(destination.name + '.unpacked')
            children[0].rename(extracted)
            destination.rmdir()
            extracted.rename(destination)
    paths[item['name']] = destination

jobs = min(os.cpu_count() or 2, 6)

def meson(name, options):
    build = builds / name
    stamp = build / '.installed'
    signature = json.dumps(options)
    if stamp.exists() and stamp.read_text() == signature:
        return
    if not (build / 'build.ninja').exists():
        run(['meson', 'setup', build, paths[name], f'--prefix={prefix}', '--libdir=lib',
             '--buildtype=release', '--wrap-mode=nodownload', '-Ddefault_library=static',
             '-Db_staticpic=true', '-Db_ndebug=true', *options])
    run(['meson', 'configure', build, '-Db_ndebug=true', *options])
    run(['meson', 'compile', '-C', build, '-j', jobs])
    run(['meson', 'install', '-C', build])
    stamp.write_text(signature)


def cmake(name, options):
    build = builds / name
    stamp = build / '.installed'
    signature = json.dumps(options)
    if stamp.exists() and stamp.read_text() == signature:
        return
    run(['cmake', '-S', paths[name], '-B', build, '-G', 'Ninja',
         f'-DCMAKE_INSTALL_PREFIX={prefix}', '-DCMAKE_BUILD_TYPE=Release',
         '-DCMAKE_POSITION_INDEPENDENT_CODE=ON', '-DBUILD_SHARED_LIBS=OFF', *options])
    run(['cmake', '--build', build, '--parallel', jobs])
    run(['cmake', '--install', build])
    stamp.write_text(signature)

(prefix / 'lib/pkgconfig').mkdir(parents=True, exist_ok=True)
shutil.copytree(paths['vulkan-headers'] / 'include', prefix / 'include', dirs_exist_ok=True)
shutil.copy2(paths['moltenvk'] / 'MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib', prefix / 'lib/libMoltenVK.dylib')
(prefix / 'lib/pkgconfig/vulkan.pc').write_text(
    f'prefix={prefix}\nlibdir=${{prefix}}/lib\nincludedir=${{prefix}}/include\n'
    'Name: Vulkan\nDescription: Pinned MoltenVK\nVersion: 1.4.310\n'
    'Libs: -L${libdir} -lMoltenVK\nCflags: -I${includedir}\n')
cmake('glslang', ['-DENABLE_OPT=OFF', '-DBUILD_EXTERNAL=OFF', '-DENABLE_GLSLANG_BINARIES=OFF',
                 '-DENABLE_SPVREMAPPER=OFF', '-DENABLE_HLSL=OFF', '-DBUILD_TESTING=OFF'])
cmake('freetype', ['-DFT_DISABLE_ZLIB=ON', '-DFT_DISABLE_BZIP2=ON', '-DFT_DISABLE_PNG=ON',
                  '-DFT_DISABLE_HARFBUZZ=ON', '-DFT_DISABLE_BROTLI=ON'])
meson('harfbuzz', ['-Dglib=disabled', '-Dgobject=disabled', '-Dcairo=disabled',
                  '-Dicu=disabled', '-Dfreetype=disabled', '-Dtests=disabled', '-Ddocs=disabled', '-Dutilities=disabled'])
meson('fribidi', ['-Ddocs=false', '-Dbin=false', '-Dtests=false'])
meson('libass', ['-Dfontconfig=disabled', '-Dcoretext=enabled', '-Dtest=disabled'])
meson('dav1d', ['-Denable_tools=false', '-Denable_tests=false'])
shutil.copytree(paths['fast-float'] / 'include', paths['libplacebo'] / '3rdparty/fast_float/include', dirs_exist_ok=True)
placebo_patch = root / 'desktop/native/mpv-host/patches/libplacebo-prefix.patch'
if not (paths['libplacebo'] / '.panorama-patch').exists():
    run(['patch', '-p1', '-i', placebo_patch], paths['libplacebo'])
    (paths['libplacebo'] / '.panorama-patch').touch()
host_import_patch = root / 'desktop/native/mpv-host/patches/libplacebo-moltenvk-host-import.patch'
if not (paths['libplacebo'] / '.panorama-host-import-patch').exists():
    run(['patch', '-p1', '-i', host_import_patch], paths['libplacebo'])
    (paths['libplacebo'] / '.panorama-host-import-patch').touch()
    for name in ('libplacebo', 'mpv'):
        (builds / name / '.installed').unlink(missing_ok=True)
meson('libplacebo', ['-Dauto_features=disabled', '-Dvulkan=enabled', '-Dvk-proc-addr=enabled',
                    f"-Dvulkan-registry={paths['vulkan-headers']}/registry/vk.xml",
                    '-Dglslang=enabled', '-Dshaderc=disabled', '-Ddemos=false', '-Dtests=false'])
ffmpeg = builds / 'ffmpeg'
if not (ffmpeg / '.installed').exists():
    ffmpeg.mkdir(exist_ok=True)
    run([paths['ffmpeg'] / 'configure', f'--prefix={prefix}', '--disable-autodetect',
         '--disable-programs', '--disable-doc', '--disable-debug', '--disable-shared', '--enable-static',
         '--enable-pic', '--enable-securetransport', '--enable-videotoolbox', '--enable-audiotoolbox',
         '--enable-libdav1d', '--enable-zlib', '--pkg-config-flags=--static'], ffmpeg)
    run(['make', f'-j{jobs}'], ffmpeg)
    run(['make', 'install'], ffmpeg)
    (ffmpeg / '.installed').touch()

patch = root / 'desktop/native/mpv-host/patches/panorama-macvk.patch'
if patch.exists():
    applied = paths['mpv'] / '.panorama-patch'
    digest = hashlib.sha256(patch.read_bytes()).hexdigest()
    if applied.exists() and applied.read_text() != digest:
        sys.exit('MPV patch changed; remove only the cached mpv source and build directories before rebuilding.')
    if not applied.exists():
        run(['patch', '-p1', '-i', patch], paths['mpv'])
        applied.write_text(digest)
audio_patch = root / 'desktop/native/mpv-host/patches/mpv-coreaudio-channel-layout.patch'
audio_applied = paths['mpv'] / '.panorama-audio-patch'
audio_digest = hashlib.sha256(audio_patch.read_bytes()).hexdigest()
if audio_applied.exists() and audio_applied.read_text() != audio_digest:
    sys.exit('MPV audio patch changed; remove only the cached mpv source and build directories before rebuilding.')
if not audio_applied.exists():
    run(['patch', '-p1', '-i', audio_patch], paths['mpv'])
    audio_applied.write_text(audio_digest)
    (builds / 'mpv' / '.installed').unlink(missing_ok=True)
meson('mpv', ['-Dauto_features=disabled', '-Ddefault_library=shared', '-Dprefer_static=true',
              '-Db_lundef=true', "-Dobjc_link_args=['-lc++', '-framework', 'Metal', '-framework', 'IOSurface']", '-Dlibmpv=true', '-Dcplayer=false', '-Dgl=enabled', '-Dgl-cocoa=enabled',
              '-Dplain-gl=enabled', '-Dvulkan=enabled', '-Dvideotoolbox-gl=enabled',
              '-Dvideotoolbox-pl=enabled', '-Dcoreaudio=enabled',
              '-Dcocoa=enabled', '-Dswift-build=disabled'])
licenses = prefix / 'licenses'
licenses.mkdir(exist_ok=True)
for name, source in paths.items():
    target = licenses / name
    target.mkdir(exist_ok=True)
    for candidate in source.rglob('*'):
        if candidate.is_file() and candidate.name.lower().startswith(('license', 'copying', 'copyright')):
            relative = candidate.relative_to(source)
            (target / relative.parent).mkdir(parents=True, exist_ok=True)
            shutil.copy2(candidate, target / relative)
shutil.copytree(root / 'desktop/native/mpv-host/macos-licenses', licenses / 'moltenvk-dependencies', dirs_exist_ok=True)
shutil.copy2(root / 'desktop/native/mpv-host/patches/libplacebo-prefix.patch', licenses / 'libplacebo-prefix.patch')
shutil.copy2(host_import_patch, licenses / host_import_patch.name)
shutil.copy2(audio_patch, licenses / audio_patch.name)
shutil.copy2(manifest_path, licenses / 'macos-libmpv.json')
if patch.exists():
    shutil.copy2(patch, licenses / patch.name)
print(f'Native runtime installed: {prefix}')
