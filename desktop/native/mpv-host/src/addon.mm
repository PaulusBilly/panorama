#import <AppKit/AppKit.h>
#import <CoreVideo/CoreVideo.h>
#import <CoreAudio/CoreAudio.h>
#import <QuartzCore/CAMetalLayer.h>
#include <mutex>
#include "macvk-surface.h"
#import <OpenGL/gl3.h>
#include <algorithm>
#include <atomic>
#include <dlfcn.h>
#include <napi.h>
#include "subtitle-cue.h"
#include "playback-state.h"
#include <mpv/client.h>
#include <mpv/render.h>
#include <mpv/render_gl.h>
#include <cmath>
#include <cstring>
#include <string>
#include <vector>

@interface PanoramaMpvView : NSOpenGLView
- (instancetype)initWithParent:(NSView *)parent;
- (void)loadURL:(const std::string &)url startSeconds:(double)startSeconds;
- (void)rememberURL:(const std::string &)url startSeconds:(double)startSeconds;
- (void)setPausedValue:(bool)paused;
- (void)seekToSeconds:(double)seconds;
- (void)setVideoBoundsX:(double)x y:(double)y width:(double)width height:(double)height;
- (void)stopPlayback;
- (int)replacePlayback:(const char **)command;
- (NSDictionary<NSString *, id> *)diagnostics;
- (mpv_handle *)mpvHandle;
- (void)shutdown;
- (void)signalMpvRender;
- (void)displayLinkDidTick;
- (void)renderPendingFrame;
- (void)rebindDisplayLink;
- (panorama_surface_state)surfaceSnapshot;
- (void)metalSwapped;
- (void)updateMetalSurface;
@end

static panorama_surface_state surfaceSnapshot(void *opaque) {
  return [(__bridge PanoramaMpvView *)opaque surfaceSnapshot];
}

static void surfaceSwapped(void *opaque) {
  [(__bridge PanoramaMpvView *)opaque metalSwapped];
}

static void *getOpenGLProcAddress(void *, const char *name) {
  return dlsym(RTLD_DEFAULT, name);
}

static void requestMpvRender(void *context) {
  PanoramaMpvView *view = (__bridge PanoramaMpvView *)context;
  [view signalMpvRender];
}

static CVReturn displayLinkCallback(
  CVDisplayLinkRef,
  const CVTimeStamp *,
  const CVTimeStamp *,
  CVOptionFlags,
  CVOptionFlags *,
  void *context
) {
  PanoramaMpvView *view = (__bridge PanoramaMpvView *)context;
  [view displayLinkDidTick];
  return kCVReturnSuccess;
}

static void requestMpvEvents(void *context) {
  PanoramaMpvView *view = (__bridge PanoramaMpvView *)context;
  dispatch_async(dispatch_get_main_queue(), ^{
    [view performSelector:@selector(readMpvEvents)];
  });
}

static NSString *readMpvString(mpv_handle *mpv, const std::string &property) {
  char *value = mpv_get_property_string(mpv, property.c_str());
  if (!value) return nil;
  NSString *result = [NSString stringWithUTF8String:value];
  mpv_free(value);
  return result;
}

static id readMpvNumber(mpv_handle *mpv, const char *property) {
  double value = 0;
  return mpv && mpv_get_property(mpv, property, MPV_FORMAT_DOUBLE, &value) >= 0 && std::isfinite(value)
    ? @(value) : [NSNull null];
}

static id readCacheForwardBytes(mpv_handle *mpv) {
  mpv_node state{};
  id result = [NSNull null];
  if (!mpv || mpv_get_property(mpv, "demuxer-cache-state", MPV_FORMAT_NODE, &state) < 0) return result;
  if (state.format == MPV_FORMAT_NODE_MAP) {
    for (int index = 0; index < state.u.list->num; index += 1) {
      const mpv_node &value = state.u.list->values[index];
      if (std::strcmp(state.u.list->keys[index], "fw-bytes") == 0 && value.format == MPV_FORMAT_INT64) {
        result = @(value.u.int64);
        break;
      }
    }
  }
  mpv_free_node_contents(&state);
  return result;
}

static NSArray<NSString *> *passthroughCodecs(NSString *name) {
  if (![name hasPrefix:@"coreaudio/"]) return @[];
  CFStringRef uid = (__bridge CFStringRef)[name substringFromIndex:10];
  AudioDeviceID device = kAudioObjectUnknown;
  AudioValueTranslation translation = { &uid, sizeof(uid), &device, sizeof(device) };
  AudioObjectPropertyAddress address = { kAudioHardwarePropertyDeviceForUID,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
  UInt32 size = sizeof(translation);
  if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, 0, nullptr,
      &size, &translation) != noErr || device == kAudioObjectUnknown) return @[];
  address = { kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput,
    kAudioObjectPropertyElementMain };
  if (AudioObjectGetPropertyDataSize(device, &address, 0, nullptr, &size) != noErr) return @[];
  std::vector<AudioStreamID> streams(size / sizeof(AudioStreamID));
  if (AudioObjectGetPropertyData(device, &address, 0, nullptr, &size, streams.data()) != noErr) return @[];
  for (AudioStreamID stream : streams) {
    address = { kAudioStreamPropertyAvailablePhysicalFormats,
      kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
    if (AudioObjectGetPropertyDataSize(stream, &address, 0, nullptr, &size) != noErr) continue;
    std::vector<AudioStreamRangedDescription> formats(size / sizeof(AudioStreamRangedDescription));
    if (AudioObjectGetPropertyData(stream, &address, 0, nullptr, &size, formats.data()) != noErr) continue;
    for (const auto &format : formats) {
      if (format.mFormat.mFormatID == kAudioFormat60958AC3 || format.mFormat.mFormatID == kAudioFormatAC3)
        return @[@"ac3"];
    }
  }
  return @[];
}

@implementation PanoramaMpvView {
  mpv_handle *_mpv;
  PlaybackState _playback;
  BOOL _modern;
  BOOL _rendererFallback;
  NSView *_backingView;
  NSView *_metalView;
  CAMetalLayer *_metalLayer;
  panorama_surface _surfaceDescriptor;
  panorama_surface_state _surfaceState;
  std::mutex _surfaceMutex;
  NSString *_loadedURL;
  double _loadedStart;
  mpv_render_context *_renderContext;
  __weak NSView *_parentView;
  NSString *_mpvVersion;
  NSString *_videoCodec;
  NSString *_hardwareDecoder;
  NSArray<NSDictionary<NSString *, id> *> *_audioDevices;
  NSString *_audioDeviceSignature;
  unsigned long long _audioOutputErrorSequence;
  BOOL _buffering;
  BOOL _renderReady;
  std::atomic<unsigned long long> _renderUpdates;
  std::atomic<unsigned long long> _renderedFrames;
  std::atomic<unsigned long long> _reportedSwaps;
  unsigned long long _endSequence;
  NSString *_endReason;
  NSString *_endError;
  CVDisplayLinkRef _displayLink;
  dispatch_queue_t _renderQueue;
  std::atomic<bool> _shutdown;
  std::atomic<bool> _pendingRender;
  std::atomic<bool> _forceRender;
  std::atomic<bool> _renderScheduled;
  std::atomic<bool> _surfaceVisible;
  std::atomic<int> _backingWidth;
  std::atomic<int> _backingHeight;
}

- (instancetype)initWithParent:(NSView *)parent {
  NSOpenGLPixelFormatAttribute attributes[] = {
    NSOpenGLPFAAccelerated,
    NSOpenGLPFADoubleBuffer,
    NSOpenGLPFAOpenGLProfile,
    NSOpenGLProfileVersion3_2Core,
    0,
  };
  NSOpenGLPixelFormat *pixelFormat = [[NSOpenGLPixelFormat alloc] initWithAttributes:attributes];
  self = [super initWithFrame:parent.bounds pixelFormat:pixelFormat];
  if (!self) return nil;

  _renderQueue = dispatch_queue_create("com.panorama.mpv-render", DISPATCH_QUEUE_SERIAL);
  _shutdown.store(false);
  _pendingRender.store(false);
  _forceRender.store(false);
  _renderScheduled.store(false);
  _surfaceVisible.store(true);
  _backingWidth.store(0);
  _backingHeight.store(0);
  _renderUpdates.store(0);
  _renderedFrames.store(0);
  _reportedSwaps.store(0);

  NSView *container = parent.superview ?: parent;
  _parentView = container;
  self.frame = parent.superview ? parent.frame : parent.bounds;
  self.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
  self.wantsBestResolutionOpenGLSurface = YES;
  if (parent.superview) {
    [container addSubview:self positioned:NSWindowBelow relativeTo:parent];
  } else {
    [container addSubview:self positioned:NSWindowBelow relativeTo:nil];
  }
  [[NSNotificationCenter defaultCenter] addObserver:self selector:@selector(windowDidChangeScreen:) name:NSWindowDidChangeScreenNotification object:self.window];
  [[NSNotificationCenter defaultCenter] addObserver:self selector:@selector(windowDidChangeScreen:) name:NSWindowDidChangeBackingPropertiesNotification object:self.window];

  _modern = !std::getenv("PANORAMA_MPV_RENDERER") ||
    std::strcmp(std::getenv("PANORAMA_MPV_RENDERER"), "opengl") != 0;
  if (_modern) {
    _metalView = [[NSView alloc] initWithFrame:self.bounds];
    _metalLayer = [CAMetalLayer layer];
    _metalLayer.opaque = YES;
    _metalLayer.wantsExtendedDynamicRangeContent = YES;
    _metalView.layer = _metalLayer;
    _metalView.wantsLayer = YES;
    _metalView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    [self addSubview:_metalView];
    _surfaceDescriptor = { 1, (__bridge void *)_metalLayer, (__bridge void *)self,
      surfaceSnapshot, surfaceSwapped };
    [self updateMetalSurface];
  }
  _mpv = mpv_create();
  if (!_mpv) {
    [self removeFromSuperview];
    return nil;
  }

  mpv_set_option_string(_mpv, "terminal", "no");
  mpv_set_option_string(_mpv, "msg-level", "all=warn");
  mpv_set_option_string(_mpv, "keep-open", "yes");
  mpv_set_option_string(_mpv, "hr-seek", "default");
  mpv_set_option_string(_mpv, "hwdec", "auto-safe");
  mpv_set_option_string(_mpv, "gpu-hwdec-interop", "auto");
  mpv_set_option_string(_mpv, "cache", "yes");
  mpv_set_option_string(_mpv, "demuxer-max-bytes", "512MiB");
  mpv_set_option_string(_mpv, "demuxer-max-back-bytes", "64MiB");
  mpv_set_option_string(_mpv, "cache-pause", "yes");
  mpv_set_option_string(_mpv, "cache-pause-wait", "2");
  mpv_set_option_string(_mpv, "cache-on-disk", "no");
  mpv_set_option_string(_mpv, "demuxer-cache-wait", "no");
  // Network resilience and buffering headroom for high-bitrate HTTP sources.
  mpv_set_option_string(_mpv, "cache-pause-initial", "yes");
  mpv_set_option_string(_mpv, "network-timeout", "60");
  mpv_set_option_string(_mpv, "audio-client-name", "Panorama");
  if (_modern) {
    mpv_set_option_string(_mpv, "vo", "gpu-next");
    mpv_set_option_string(_mpv, "gpu-api", "vulkan");
    mpv_set_option_string(_mpv, "gpu-context", "panorama-macvk");
    mpv_set_option_string(_mpv, "target-colorspace-hint", "auto");
    const std::string descriptor = std::to_string(reinterpret_cast<uintptr_t>(&_surfaceDescriptor));
    mpv_set_option_string(_mpv, "wid", descriptor.c_str());
  }
  if (mpv_initialize(_mpv) < 0) {
    mpv_terminate_destroy(_mpv);
    _mpv = nullptr;
    [self removeFromSuperview];
    return nil;
  }

  if (!_modern) mpv_set_property_string(_mpv, "vo", "libmpv");
  _renderReady = _modern;
  mpv_observe_property(_mpv, 1, "mpv-version", MPV_FORMAT_STRING);
  mpv_observe_property(_mpv, 2, "video-codec", MPV_FORMAT_STRING);
  mpv_observe_property(_mpv, 3, "hwdec-current", MPV_FORMAT_STRING);
  mpv_observe_property(_mpv, 4, "paused-for-cache", MPV_FORMAT_FLAG);
  mpv_request_log_messages(_mpv, "error");
  mpv_set_wakeup_callback(_mpv, requestMpvEvents, (__bridge void *)self);
  _backingView = [[NSView alloc] initWithFrame:container.bounds];
  _backingView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
  _backingView.wantsLayer = YES;
  _backingView.layer.opaque = YES;
  _backingView.layer.backgroundColor = NSColor.blackColor.CGColor;
  [container addSubview:_backingView positioned:NSWindowBelow relativeTo:self];
  [self setNeedsDisplay:YES];
  return self;
}

- (void)prepareOpenGL {
  if (_modern) return;
  [super prepareOpenGL];
  if (_shutdown.load() || !_mpv || _renderContext) return;
  [[self openGLContext] makeCurrentContext];
  GLint swapInterval = 1;
  [[self openGLContext] setValues:&swapInterval forParameter:NSOpenGLContextParameterSwapInterval];

  mpv_opengl_init_params openGLParams = { getOpenGLProcAddress, nullptr };
  mpv_render_param params[] = {
    { MPV_RENDER_PARAM_API_TYPE, const_cast<char *>(MPV_RENDER_API_TYPE_OPENGL) },
    { MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &openGLParams },
    { MPV_RENDER_PARAM_INVALID, nullptr },
  };
  if (mpv_render_context_create(&_renderContext, _mpv, params) < 0) return;
  _renderReady = YES;
  mpv_render_context_set_update_callback(_renderContext, requestMpvRender, (__bridge void *)self);
  if (CVDisplayLinkCreateWithActiveCGDisplays(&_displayLink) == kCVReturnSuccess && _displayLink) {
    CVDisplayLinkSetOutputCallback(_displayLink, displayLinkCallback, (__bridge void *)self);
    [self rebindDisplayLink];
    CVDisplayLinkStart(_displayLink);
  }
  [self reshape];
}

- (void)drawRect:(NSRect)dirtyRect {
  if (_modern) return;
  [super drawRect:dirtyRect];
  if (_shutdown.load() || !_renderContext) return;
  _forceRender.store(true);
  [self signalMpvRender];
}

- (void)reshape {
  if (_modern) { [self updateMetalSurface]; return; }
  [super reshape];
  NSSize backingSize = [self convertSizeToBacking:self.bounds.size];
  _backingWidth.store(std::max(0, static_cast<int>(backingSize.width)));
  _backingHeight.store(std::max(0, static_cast<int>(backingSize.height)));
  _forceRender.store(true);
  [self signalMpvRender];
}

- (void)windowDidChangeScreen:(NSNotification *)notification {
  if (_shutdown.load()) return;
  if (_modern) { [self updateMetalSurface]; return; }
  [[self openGLContext] update];
  [self rebindDisplayLink];
  [self reshape];
}

- (void)rebindDisplayLink {
  if (!_displayLink || ![self openGLContext] || ![self pixelFormat]) return;
  CVDisplayLinkSetCurrentCGDisplayFromOpenGLContext(
    _displayLink,
    [[self openGLContext] CGLContextObj],
    [[self pixelFormat] CGLPixelFormatObj]
  );
}

- (void)signalMpvRender {
  if (_shutdown.load()) return;
  _pendingRender.store(true);
}

- (void)displayLinkDidTick {
  if (_shutdown.load() || !_surfaceVisible.load()) return;
  if (!_pendingRender.load() && !_forceRender.load()) return;
  bool expected = false;
  if (!_renderScheduled.compare_exchange_strong(expected, true)) return;
  dispatch_async(_renderQueue, ^{
    [self renderPendingFrame];
    _renderScheduled.store(false);
  });
}

- (void)renderPendingFrame {
  if (_shutdown.load() || !_renderContext || !_surfaceVisible.load()) return;
  const bool forceRender = _forceRender.exchange(false);
  _pendingRender.exchange(false);
  NSOpenGLContext *openGLContext = [self openGLContext];
  CGLContextObj cglContext = [openGLContext CGLContextObj];
  if (!cglContext) return;
  CGLLockContext(cglContext);
  [openGLContext makeCurrentContext];
  uint64_t updateFlags = mpv_render_context_update(_renderContext);
  _renderUpdates.fetch_add(1);
  if (!(updateFlags & MPV_RENDER_UPDATE_FRAME) && !forceRender) {
    CGLUnlockContext(cglContext);
    return;
  }
  const int width = _backingWidth.load();
  const int height = _backingHeight.load();
  if (width <= 0 || height <= 0 || !_surfaceVisible.load()) {
    CGLUnlockContext(cglContext);
    return;
  }
  GLint framebuffer = 0;
  glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &framebuffer);
  mpv_opengl_fbo target = {
    framebuffer,
    width,
    height,
    0,
  };
  int flipY = 1;
  mpv_render_param params[] = {
    { MPV_RENDER_PARAM_OPENGL_FBO, &target },
    { MPV_RENDER_PARAM_FLIP_Y, &flipY },
    { MPV_RENDER_PARAM_INVALID, nullptr },
  };
  const auto frameToken = _playback.FrameToken();
  mpv_render_context_render(_renderContext, params);
  _renderedFrames.fetch_add(1);
  [openGLContext flushBuffer];
  mpv_render_context_report_swap(_renderContext);
  _reportedSwaps.fetch_add(1);
  _playback.Frame(frameToken);
  CGLUnlockContext(cglContext);
}

- (panorama_surface_state)surfaceSnapshot {
  std::lock_guard<std::mutex> lock(_surfaceMutex);
  return _surfaceState;
}

- (void)metalSwapped {
  _playback.Frame(_playback.FrameToken());
  _renderedFrames.fetch_add(1);
  _reportedSwaps.fetch_add(1);
}

- (void)updateMetalSurface {
  if (!_modern || _shutdown.load()) return;
  NSSize size = [self convertSizeToBacking:self.bounds.size];
  NSScreen *screen = self.window.screen;
  double headroom = screen ? screen.maximumExtendedDynamicRangeColorComponentValue : 1;
  double scale = self.window.backingScaleFactor ?: 1;
  double fps = screen ? screen.maximumFramesPerSecond : 60;
  _metalLayer.contentsScale = scale;
  _metalLayer.drawableSize = CGSizeMake(std::max(1.0, size.width), std::max(1.0, size.height));
  std::lock_guard<std::mutex> lock(_surfaceMutex);
  panorama_surface_state next = { static_cast<int>(size.width), static_cast<int>(size.height),
    !self.hidden, fps, scale, std::max(1.0, headroom), _surfaceState.generation };
  if (next.width != _surfaceState.width || next.height != _surfaceState.height ||
      next.visible != _surfaceState.visible || next.scale != _surfaceState.scale ||
      next.headroom != _surfaceState.headroom || next.fps != _surfaceState.fps) next.generation += 1;
  _surfaceState = next;
}

- (void)readMpvEvents {
  if (_shutdown.load() || !_mpv) return;
  while (true) {
    mpv_event *event = mpv_wait_event(_mpv, 0);
    if (!event || event->event_id == MPV_EVENT_NONE) break;
    const bool restarted = _playback.Event(*event);
    if (restarted) {
      if (_modern) {
        const char *redraw[] = { "osd-overlay", "2147483647", "ass-events", "", nullptr };
        mpv_command_async(_mpv, 0, redraw);
      } else {
        _forceRender.store(true);
        [self signalMpvRender];
      }
    }
    if (event->event_id == MPV_EVENT_LOG_MESSAGE && event->data) {
      const auto *message = static_cast<mpv_event_log_message *>(event->data);
      if (message->prefix && message->text && std::strcmp(message->prefix, "ao") == 0 &&
          std::strstr(message->text, "Failed to initialize audio driver")) _audioOutputErrorSequence += 1;
      continue;
    }
    if (event->event_id == MPV_EVENT_END_FILE && event->data) {
      mpv_event_end_file *endFile = static_cast<mpv_event_end_file *>(event->data);
      if (endFile->playlist_entry_id != _playback.Snapshot().owned_entry) continue;
      if (_modern && !_rendererFallback && endFile->error == MPV_ERROR_VO_INIT_FAILED) {
        _rendererFallback = YES;
        _modern = NO;
        [_metalView removeFromSuperview];
        mpv_set_property_string(_mpv, "vo", "libmpv");
        mpv_set_property_string(_mpv, "wid", "-1");
        [self prepareOpenGL];
        if (_renderContext && _loadedURL) {
          int paused = 0;
          mpv_get_property(_mpv, "pause", MPV_FORMAT_FLAG, &paused);
          [self loadURL:std::string([_loadedURL UTF8String]) startSeconds:_loadedStart];
          mpv_set_property(_mpv, "pause", MPV_FORMAT_FLAG, &paused);
          continue;
        }
      }
      _endSequence += 1;
      if (endFile->reason == MPV_END_FILE_REASON_EOF) _endReason = @"eof";
      else if (endFile->reason == MPV_END_FILE_REASON_ERROR) _endReason = @"error";
      else if (endFile->reason == MPV_END_FILE_REASON_STOP) _endReason = @"stop";
      else if (endFile->reason == MPV_END_FILE_REASON_QUIT) _endReason = @"quit";
      else _endReason = @"other";
      _endError = endFile->error < 0
        ? [NSString stringWithUTF8String:mpv_error_string(endFile->error)]
        : nil;
      continue;
    }
    if (event->event_id != MPV_EVENT_PROPERTY_CHANGE || !event->data) continue;
    mpv_event_property *property = static_cast<mpv_event_property *>(event->data);
    if (!property->name || !property->data) continue;
    if (property->format == MPV_FORMAT_STRING) {
      char *value = *static_cast<char **>(property->data);
      NSString *stringValue = value ? [NSString stringWithUTF8String:value] : nil;
      if (std::strcmp(property->name, "mpv-version") == 0) _mpvVersion = stringValue;
      if (std::strcmp(property->name, "video-codec") == 0) _videoCodec = stringValue;
      if (std::strcmp(property->name, "hwdec-current") == 0) _hardwareDecoder = stringValue;
    } else if (property->format == MPV_FORMAT_FLAG && std::strcmp(property->name, "paused-for-cache") == 0) {
      _buffering = *static_cast<int *>(property->data) != 0;
    }
  }
}

- (void)rememberURL:(const std::string &)url startSeconds:(double)startSeconds {
  _loadedURL = [NSString stringWithUTF8String:url.c_str()];
  _loadedStart = startSeconds;
}

- (int)replacePlayback:(const char **)command {
  mpv_set_property_string(_mpv, "cache-pause-wait", "2");
  int result = 0;
  const bool owned = _playback.Replace([&] {
    mpv_node reply{};
    result = mpv_command_ret(_mpv, command, &reply);
    int64_t entry = -1;
    if (result >= 0 && reply.format == MPV_FORMAT_NODE_MAP) {
      for (int index = 0; index < reply.u.list->num; index += 1) {
        const auto &value = reply.u.list->values[index];
        if (std::strcmp(reply.u.list->keys[index], "playlist_entry_id") == 0 && value.format == MPV_FORMAT_INT64) entry = value.u.int64;
      }
    }
    mpv_free_node_contents(&reply);
    return entry;
  });
  if (owned && _playback.Snapshot().decoded_ready) {
    if (_modern) {
      const char *redraw[] = { "osd-overlay", "2147483647", "ass-events", "", nullptr };
      mpv_command_async(_mpv, 0, redraw);
    } else {
      _forceRender.store(true);
      [self signalMpvRender];
    }
  }
  return owned ? result : MPV_ERROR_COMMAND;
}

- (void)loadURL:(const std::string &)url startSeconds:(double)startSeconds {
  if (_shutdown.load() || !_mpv) return;
  _loadedURL = [NSString stringWithUTF8String:url.c_str()];
  _loadedStart = startSeconds;
  _endReason = nil;
  _endError = nil;
  std::string start = "start=+" + std::to_string(startSeconds);
  const char *command[] = { "loadfile", url.c_str(), "replace", "-1", start.c_str(), nullptr };
  [self replacePlayback:command];
  int paused = 0;
  mpv_set_property(_mpv, "pause", MPV_FORMAT_FLAG, &paused);
}

- (void)setPausedValue:(bool)paused {
  if (_shutdown.load() || !_mpv) return;
  int value = paused ? 1 : 0;
  mpv_set_property(_mpv, "pause", MPV_FORMAT_FLAG, &value);
}

- (void)seekToSeconds:(double)seconds {
  if (_shutdown.load() || !_mpv) return;
  std::string target = std::to_string(seconds);
  const char *command[] = { "seek", target.c_str(), "absolute+exact", nullptr };
  mpv_set_property_string(_mpv, "cache-pause-wait", "2");
  _playback.Seek([&](uint64_t token) { return mpv_command_async(_mpv, token, command); });
}

- (void)setVideoBoundsX:(double)x y:(double)y width:(double)width height:(double)height {
  NSView *parent = _parentView;
  if (!parent) return;
  self.hidden = width <= 0 || height <= 0;
  _surfaceVisible.store(!self.hidden);
  if (self.hidden) { if (_modern) [self updateMetalSurface]; return; }
  double nativeY = parent.bounds.size.height - y - height;
  self.frame = NSMakeRect(x, nativeY, width, height);
  [self reshape];
}

- (void)stopPlayback {
  if (_shutdown.load() || !_mpv) return;
  _playback.Stop();
  const char *command[] = { "stop", nullptr };
  mpv_command(_mpv, command);
}

- (NSDictionary<NSString *, id> *)diagnostics {
  if (_modern) [self updateMetalSurface];
  double timeSeconds = 0;
  double durationSeconds = 0;
  double volume = 0;
  double subtitleScale = 0;
  double subtitlePosition = 0;
  double subtitleDelay = 0;
  double speed = 0;
  int paused = 0;
  int seeking = 0;
  int eofReached = 0;
  int64_t videoWidth = 0;
  int64_t videoHeight = 0;
  double sourceFps = 0;
  double displayFps = 0;
  int64_t frameDropCount = 0;
  int64_t decoderFrameDropCount = 0;
  int64_t mistimedFrameCount = 0;
  int64_t delayedFrameCount = 0;
  BOOL hasTime = _mpv && mpv_get_property(_mpv, "time-pos", MPV_FORMAT_DOUBLE, &timeSeconds) >= 0;
  BOOL hasDuration = _mpv && mpv_get_property(_mpv, "duration", MPV_FORMAT_DOUBLE, &durationSeconds) >= 0;
  BOOL hasVolume = _mpv && mpv_get_property(_mpv, "volume", MPV_FORMAT_DOUBLE, &volume) >= 0;
  BOOL hasSubtitleScale = _mpv && mpv_get_property(_mpv, "sub-scale", MPV_FORMAT_DOUBLE, &subtitleScale) >= 0;
  BOOL hasSubtitlePosition = _mpv && mpv_get_property(_mpv, "sub-pos", MPV_FORMAT_DOUBLE, &subtitlePosition) >= 0;
  BOOL hasSubtitleDelay = _mpv && mpv_get_property(_mpv, "sub-delay", MPV_FORMAT_DOUBLE, &subtitleDelay) >= 0;
  BOOL hasSpeed = _mpv && mpv_get_property(_mpv, "speed", MPV_FORMAT_DOUBLE, &speed) >= 0;
  if (_mpv) {
    mpv_get_property(_mpv, "pause", MPV_FORMAT_FLAG, &paused);
    mpv_get_property(_mpv, "seeking", MPV_FORMAT_FLAG, &seeking);
    mpv_get_property(_mpv, "eof-reached", MPV_FORMAT_FLAG, &eofReached);
  }
  BOOL hasVideoWidth = _mpv && mpv_get_property(_mpv, "video-params/w", MPV_FORMAT_INT64, &videoWidth) >= 0;
  BOOL hasVideoHeight = _mpv && mpv_get_property(_mpv, "video-params/h", MPV_FORMAT_INT64, &videoHeight) >= 0;
  BOOL hasSourceFps = _mpv && mpv_get_property(_mpv, "estimated-vf-fps", MPV_FORMAT_DOUBLE, &sourceFps) >= 0;
  BOOL hasDisplayFps = _mpv && mpv_get_property(_mpv, "display-fps", MPV_FORMAT_DOUBLE, &displayFps) >= 0;
  BOOL hasFrameDropCount = _mpv && mpv_get_property(_mpv, "frame-drop-count", MPV_FORMAT_INT64, &frameDropCount) >= 0;
  BOOL hasDecoderFrameDropCount = _mpv && mpv_get_property(_mpv, "decoder-frame-drop-count", MPV_FORMAT_INT64, &decoderFrameDropCount) >= 0;
  BOOL hasMistimedFrameCount = _mpv && mpv_get_property(_mpv, "mistimed-frame-count", MPV_FORMAT_INT64, &mistimedFrameCount) >= 0;
  BOOL hasDelayedFrameCount = _mpv && mpv_get_property(_mpv, "vo-delayed-frame-count", MPV_FORMAT_INT64, &delayedFrameCount) >= 0;
  int64_t trackCount = 0;
  NSMutableArray<NSDictionary<NSString *, id> *> *tracks = [NSMutableArray array];
  if (_mpv && mpv_get_property(_mpv, "track-list/count", MPV_FORMAT_INT64, &trackCount) >= 0) {
    for (int64_t index = 0; index < trackCount; index += 1) {
      std::string prefix = "track-list/" + std::to_string(index) + "/";
      int64_t trackId = 0;
      int selected = 0;
      int forced = 0;
      int hearingImpaired = 0;
      BOOL hasTrackId = mpv_get_property(_mpv, (prefix + "id").c_str(), MPV_FORMAT_INT64, &trackId) >= 0;
      mpv_get_property(_mpv, (prefix + "selected").c_str(), MPV_FORMAT_FLAG, &selected);
      mpv_get_property(_mpv, (prefix + "forced").c_str(), MPV_FORMAT_FLAG, &forced);
      mpv_get_property(_mpv, (prefix + "hearing-impaired").c_str(), MPV_FORMAT_FLAG, &hearingImpaired);
      [tracks addObject:@{
        @"id": hasTrackId ? @(trackId) : [NSNull null],
        @"type": readMpvString(_mpv, prefix + "type") ?: [NSNull null],
        @"language": readMpvString(_mpv, prefix + "lang") ?: [NSNull null],
        @"title": readMpvString(_mpv, prefix + "title") ?: [NSNull null],
        @"selected": @(selected != 0),
        @"forced": @(forced != 0),
        @"hearingImpaired": @(hearingImpaired != 0),
        @"codec": readMpvString(_mpv, prefix + "codec") ?: [NSNull null],
      }];
    }
  }
  NSString *signature = readMpvString(_mpv, "audio-device-list");
  if (!_audioDevices || ![signature isEqualToString:_audioDeviceSignature]) {
    NSMutableArray *devices = [NSMutableArray array];
    int64_t count = 0;
    if (_mpv && mpv_get_property(_mpv, "audio-device-list/count", MPV_FORMAT_INT64, &count) >= 0) {
      for (int64_t index = 0; index < count; index += 1) {
        std::string prefix = "audio-device-list/" + std::to_string(index) + "/";
        NSString *name = readMpvString(_mpv, prefix + "name");
        NSString *description = readMpvString(_mpv, prefix + "description");
        if (name && description) [devices addObject:@{ @"name": name, @"description": description,
          @"supportedPassthroughCodecs": passthroughCodecs(name) }];
      }
    }
    _audioDeviceSignature = signature;
    _audioDevices = devices;
  }
  const auto playback = _playback.Sample(hasTime ? timeSeconds : NAN, paused != 0, _buffering, seeking != 0);
  return @{
    @"audioDevices": _audioDevices,
    @"audioOutputErrorSequence": @(_audioOutputErrorSequence),
    @"audioOutputFormat": readMpvString(_mpv, "audio-out-params/format") ?: [NSNull null],
    @"audioOutputDriver": readMpvString(_mpv, "current-ao") ?: [NSNull null],
    @"rendererBackend": _modern ? @"macvk" : @"opengl",
    @"rendererFallbackReason": _rendererFallback ? @"initialization-failed" : [NSNull null],
    @"videoOutputPrimaries": readMpvString(_mpv, "video-out-params/primaries") ?: [NSNull null],
    @"videoOutputTransferFunction": readMpvString(_mpv, "video-out-params/gamma") ?: [NSNull null],
    @"mpvVersion": _mpvVersion ?: [NSNull null],
    @"videoCodec": readMpvString(_mpv, "video-codec") ?: [NSNull null],
    @"hardwareDecoder": readMpvString(_mpv, "hwdec-current") ?: [NSNull null],
    @"buffering": @(_buffering),
    @"cacheSeconds": readMpvNumber(_mpv, "demuxer-cache-duration"),
    @"cacheEndSeconds": readMpvNumber(_mpv, "demuxer-cache-time"),
    @"cacheBufferingPercent": readMpvNumber(_mpv, "cache-buffering-state"),
    @"cacheForwardBytes": readCacheForwardBytes(_mpv),
    @"inputBytesPerSecond": readMpvNumber(_mpv, "cache-speed"),
    @"audioSampleRate": readMpvNumber(_mpv, "audio-params/samplerate"),
    @"audioOutputSampleRate": readMpvNumber(_mpv, "audio-out-params/samplerate"),
    @"audioCodec": readMpvString(_mpv, "audio-codec-name") ?: [NSNull null],
    @"audioChannels": readMpvString(_mpv, "audio-params/channels") ?: [NSNull null],
    @"audioOutputChannels": readMpvString(_mpv, "audio-out-params/channels") ?: [NSNull null],
    @"videoPixelFormat": readMpvString(_mpv, "video-params/pixelformat") ?: [NSNull null],
    @"videoColorPrimaries": readMpvString(_mpv, "video-params/primaries") ?: [NSNull null],
    @"videoTransferFunction": readMpvString(_mpv, "video-params/gamma") ?: [NSNull null],
    @"timeSeconds": hasTime ? @(timeSeconds) : [NSNull null],
    @"durationSeconds": hasDuration ? @(durationSeconds) : [NSNull null],
    @"path": readMpvString(_mpv, "path") ?: [NSNull null],
    @"ffmpegVersion": readMpvString(_mpv, "ffmpeg-version") ?: [NSNull null],
    @"paused": @(paused != 0),
    @"seeking": @(seeking != 0),
    @"eofReached": @(eofReached != 0),
    @"volume": hasVolume ? @(volume) : [NSNull null],
    @"selectedAudioId": readMpvString(_mpv, "aid") ?: [NSNull null],
    @"selectedVideoId": readMpvString(_mpv, "vid") ?: [NSNull null],
    @"selectedSubtitleId": readMpvString(_mpv, "sid") ?: [NSNull null],
    @"subtitleScale": hasSubtitleScale ? @(subtitleScale) : [NSNull null],
    @"subtitlePosition": hasSubtitlePosition ? @(subtitlePosition) : [NSNull null],
    @"subtitleDelay": hasSubtitleDelay ? @(subtitleDelay) : [NSNull null],
    @"speed": hasSpeed ? @(speed) : [NSNull null],
    @"videoWidth": hasVideoWidth ? @(videoWidth) : [NSNull null],
    @"videoHeight": hasVideoHeight ? @(videoHeight) : [NSNull null],
    @"sourceFps": hasSourceFps ? @(sourceFps) : [NSNull null],
    @"displayFps": hasDisplayFps ? @(displayFps) : [NSNull null],
    @"frameDropCount": hasFrameDropCount ? @(frameDropCount) : [NSNull null],
    @"decoderFrameDropCount": hasDecoderFrameDropCount ? @(decoderFrameDropCount) : [NSNull null],
    @"mistimedFrameCount": hasMistimedFrameCount ? @(mistimedFrameCount) : [NSNull null],
    @"delayedFrameCount": hasDelayedFrameCount ? @(delayedFrameCount) : [NSNull null],
    @"endSequence": @(_endSequence),
    @"endReason": _endReason ?: [NSNull null],
    @"endError": _endError ?: [NSNull null],
    @"tracks": tracks,
    @"ownedEntryId": playback.owned_entry < 0 ? [NSNull null] : @(playback.owned_entry),
    @"loadGeneration": @(playback.load_generation),
    @"fileStarted": @(playback.file_started),
    @"fileLoaded": @(playback.file_loaded),
    @"decodedReady": @(playback.decoded_ready),
    @"presented": @(playback.presented),
    @"presentationEvidence": @"native-swap",
    @"moving": @(playback.moving),
    @"restartSequence": @(playback.restart_sequence),
    @"seekGeneration": @(playback.seek_generation),
    @"restartedSeekGeneration": @(playback.restarted_seek_generation),
    @"presentedSeekGeneration": @(playback.presented_seek_generation),
    @"renderReady": @(_renderReady),
    @"renderUpdates": _modern ? [NSNull null] : @(_renderUpdates.load()),
    @"renderedFrames": @(_renderedFrames.load()),
    @"reportedSwaps": @(_reportedSwaps.load()),
  };
}

- (mpv_handle *)mpvHandle {
  return _mpv;
}

- (void)shutdown {
  if (_shutdown.exchange(true)) return;
  [[NSNotificationCenter defaultCenter] removeObserver:self];
  if (_mpv) mpv_set_wakeup_callback(_mpv, nullptr, nullptr);
  if (_renderContext) mpv_render_context_set_update_callback(_renderContext, nullptr, nullptr);
  if (_displayLink) {
    CVDisplayLinkStop(_displayLink);
    CVDisplayLinkRelease(_displayLink);
    _displayLink = nullptr;
  }
  dispatch_sync(_renderQueue, ^{
    if (_renderContext) {
      mpv_render_context_free(_renderContext);
      _renderContext = nullptr;
    }
    [NSOpenGLContext clearCurrentContext];
  });
  if (_mpv) {
    mpv_terminate_destroy(_mpv);
    _mpv = nullptr;
  }
  [self removeFromSuperview];
  [_backingView removeFromSuperview];
}

- (void)dealloc {
  [self shutdown];
}

@end

class NativeMpvHost : public Napi::ObjectWrap<NativeMpvHost> {
 public:
  static Napi::Object Init(Napi::Env env, Napi::Object exports) {
    Napi::Function constructor = DefineClass(env, "NativeMpvHost", {
      InstanceMethod("load", &NativeMpvHost::Load),
      InstanceMethod("setPaused", &NativeMpvHost::SetPaused),
      InstanceMethod("seek", &NativeMpvHost::Seek),
      InstanceMethod("setBounds", &NativeMpvHost::SetBounds),
      InstanceMethod("command", &NativeMpvHost::Command),
      InstanceMethod("setProperty", &NativeMpvHost::SetProperty),
      InstanceMethod("stop", &NativeMpvHost::Stop),
      InstanceMethod("getDiagnostics", &NativeMpvHost::GetDiagnostics),
      InstanceMethod("onSubtitleCue", &NativeMpvHost::OnSubtitleCue),
      InstanceMethod("destroy", &NativeMpvHost::Destroy),
    });
    exports.Set("NativeMpvHost", constructor);
    return exports;
  }

  explicit NativeMpvHost(const Napi::CallbackInfo &info) : Napi::ObjectWrap<NativeMpvHost>(info) {
    Napi::Env env = info.Env();
    if (info.Length() != 1 || !info[0].IsBuffer()) {
      Napi::TypeError::New(env, "A native window handle buffer is required.").ThrowAsJavaScriptException();
      return;
    }
    Napi::Buffer<uint8_t> handle = info[0].As<Napi::Buffer<uint8_t>>();
    if (handle.Length() < sizeof(void *)) {
      Napi::TypeError::New(env, "The native window handle is invalid.").ThrowAsJavaScriptException();
      return;
    }
    void *pointer = nullptr;
    std::memcpy(&pointer, handle.Data(), sizeof(void *));
    NSView *parent = (__bridge NSView *)pointer;
    view_ = [[PanoramaMpvView alloc] initWithParent:parent];
    if (!view_) {
      Napi::Error::New(env, "Unable to initialize libmpv.").ThrowAsJavaScriptException();
    } else {
      subtitle_bridge_ = std::make_unique<PanoramaSubtitleBridge>([view_ mpvHandle], PanoramaSubtitleApi{ mpv_create_client, mpv_destroy, mpv_observe_property, mpv_wait_event, mpv_get_property, mpv_get_property_string, mpv_set_property_string, mpv_free, mpv_free_node_contents, mpv_wakeup });
    }
  }

  ~NativeMpvHost() override {
    subtitle_bridge_.reset();
    if (view_) [view_ shutdown];
  }

 private:
  __strong PanoramaMpvView *view_ = nil;
  std::unique_ptr<PanoramaSubtitleBridge> subtitle_bridge_;

  void OnSubtitleCue(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1) return;
    subtitle_bridge_->Listen(info.Env(), info[0]);
  }

  void Load(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 2 || !info[0].IsString() || !info[1].IsNumber()) return;
    subtitle_bridge_->Replace([&] {
      [view_ loadURL:info[0].As<Napi::String>().Utf8Value() startSeconds:info[1].As<Napi::Number>().DoubleValue()];
      return 0;
    });
  }

  void SetPaused(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1 || !info[0].IsBoolean()) return;
    [view_ setPausedValue:info[0].As<Napi::Boolean>().Value()];
  }

  void Seek(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1 || !info[0].IsNumber()) return;
    subtitle_bridge_->Seek(info[0].As<Napi::Number>().DoubleValue());
    [view_ seekToSeconds:info[0].As<Napi::Number>().DoubleValue()];
  }

  void SetBounds(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || (info.Length() != 4 && info.Length() != 5)) return;
    for (size_t index = 0; index < info.Length(); index += 1) {
      if (!info[index].IsNumber()) return;
    }
    [view_ setVideoBoundsX:info[0].As<Napi::Number>().DoubleValue()
                           y:info[1].As<Napi::Number>().DoubleValue()
                       width:info[2].As<Napi::Number>().DoubleValue()
                      height:info[3].As<Napi::Number>().DoubleValue()];
  }

  void Command(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1 || !info[0].IsArray()) return;
    Napi::Array input = info[0].As<Napi::Array>();
    std::vector<std::string> values;
    std::vector<const char *> command;
    values.reserve(input.Length());
    command.reserve(input.Length() + 1);
    for (uint32_t index = 0; index < input.Length(); index += 1) {
      Napi::Value value = input.Get(index);
      if (!value.IsString()) return;
      values.push_back(value.As<Napi::String>().Utf8Value());
    }
    for (const std::string &value : values) command.push_back(value.c_str());
    command.push_back(nullptr);
    if (values.size() >= 2 && values.front() == "loadfile") {
      double start = 0;
      for (const auto &value : values) {
        if (value.starts_with("start=+")) {
          try { start = std::stod(value.substr(7)); } catch (...) { start = 0; }
        }
      }
      [view_ rememberURL:values[1] startSeconds:start];
    }
    if (!values.empty() && values.front() == "sub-add") {
      mpv_command_async([view_ mpvHandle], 0, command.data());
      return;
    }
    if (!values.empty() && values.front() == "loadfile") { subtitle_bridge_->Replace([&] { return [view_ replacePlayback:command.data()]; }); return; }
    if (!values.empty() && values.front() == "stop") { subtitle_bridge_->Clear(); [view_ stopPlayback]; return; }
    mpv_command([view_ mpvHandle], command.data());
  }

  void SetProperty(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 2 || !info[0].IsString()) return;
    std::string name = info[0].As<Napi::String>().Utf8Value();
    std::string value;
    if (info[1].IsBoolean()) value = info[1].As<Napi::Boolean>().Value() ? "yes" : "no";
    else if (info[1].IsNumber()) value = std::to_string(info[1].As<Napi::Number>().DoubleValue());
    else if (info[1].IsString()) value = info[1].As<Napi::String>().Utf8Value();
    else if (info[1].IsNull()) value = "no";
    else return;
    if (name == "sid") subtitle_bridge_->Select(value);
    if (mpv_set_property_string([view_ mpvHandle], name.c_str(), value.c_str()) < 0)
      Napi::Error::New(info.Env(), "Unable to apply native playback setting.").ThrowAsJavaScriptException();
  }

  void Stop(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env())) return;
    subtitle_bridge_->Clear();
    [view_ stopPlayback];
  }

  Napi::Value GetDiagnostics(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env())) return info.Env().Undefined();
    NSDictionary<NSString *, id> *values = [view_ diagnostics];
    Napi::Object diagnostics = Napi::Object::New(info.Env());
    NSArray<NSDictionary<NSString *, id> *> *deviceValues = values[@"audioDevices"];
    Napi::Array devices = Napi::Array::New(info.Env(), deviceValues.count);
    for (NSUInteger index = 0; index < deviceValues.count; index += 1) {
      Napi::Object device = Napi::Object::New(info.Env());
      SetNullableString(device, "name", deviceValues[index][@"name"]);
      SetNullableString(device, "description", deviceValues[index][@"description"]);
      NSArray<NSString *> *codecValues = deviceValues[index][@"supportedPassthroughCodecs"];
      Napi::Array codecs = Napi::Array::New(info.Env(), codecValues.count);
      for (NSUInteger codec = 0; codec < codecValues.count; codec += 1)
        codecs.Set(codec, Napi::String::New(info.Env(), [codecValues[codec] UTF8String]));
      device.Set("supportedPassthroughCodecs", codecs);
      devices.Set(index, device);
    }
    diagnostics.Set("audioDevices", devices);
    SetNullableNumber(diagnostics, "audioOutputErrorSequence", values[@"audioOutputErrorSequence"]);
    for (const char *key : { "audioOutputFormat", "audioOutputDriver", "rendererBackend",
        "rendererFallbackReason", "videoOutputPrimaries", "videoOutputTransferFunction" })
      SetNullableString(diagnostics, key, values[[NSString stringWithUTF8String:key]]);
    SetNullableString(diagnostics, "mpvVersion", values[@"mpvVersion"]);
    SetNullableString(diagnostics, "videoCodec", values[@"videoCodec"]);
    SetNullableString(diagnostics, "hardwareDecoder", values[@"hardwareDecoder"]);
    diagnostics.Set("buffering", [values[@"buffering"] boolValue]);
    SetNullableNumber(diagnostics, "cacheSeconds", values[@"cacheSeconds"]);
    SetNullableNumber(diagnostics, "cacheEndSeconds", values[@"cacheEndSeconds"]);
    SetNullableNumber(diagnostics, "cacheBufferingPercent", values[@"cacheBufferingPercent"]);
    SetNullableNumber(diagnostics, "cacheForwardBytes", values[@"cacheForwardBytes"]);
    SetNullableNumber(diagnostics, "inputBytesPerSecond", values[@"inputBytesPerSecond"]);
    SetNullableNumber(diagnostics, "audioSampleRate", values[@"audioSampleRate"]);
    SetNullableNumber(diagnostics, "audioOutputSampleRate", values[@"audioOutputSampleRate"]);
    SetNullableString(diagnostics, "audioCodec", values[@"audioCodec"]);
    SetNullableString(diagnostics, "audioChannels", values[@"audioChannels"]);
    SetNullableString(diagnostics, "audioOutputChannels", values[@"audioOutputChannels"]);
    SetNullableString(diagnostics, "videoPixelFormat", values[@"videoPixelFormat"]);
    SetNullableString(diagnostics, "videoColorPrimaries", values[@"videoColorPrimaries"]);
    SetNullableString(diagnostics, "videoTransferFunction", values[@"videoTransferFunction"]);
    id time = values[@"timeSeconds"];
    id duration = values[@"durationSeconds"];
    diagnostics.Set("timeSeconds", time == [NSNull null] ? info.Env().Null() : Napi::Number::New(info.Env(), [time doubleValue]));
    diagnostics.Set("durationSeconds", duration == [NSNull null] ? info.Env().Null() : Napi::Number::New(info.Env(), [duration doubleValue]));
    SetNullableString(diagnostics, "path", values[@"path"]);
    SetNullableString(diagnostics, "ffmpegVersion", values[@"ffmpegVersion"]);
    diagnostics.Set("paused", [values[@"paused"] boolValue]);
    diagnostics.Set("seeking", [values[@"seeking"] boolValue]);
    diagnostics.Set("eofReached", [values[@"eofReached"] boolValue]);
    SetNullableNumber(diagnostics, "volume", values[@"volume"]);
    SetNullableString(diagnostics, "selectedAudioId", values[@"selectedAudioId"]);
    SetNullableString(diagnostics, "selectedVideoId", values[@"selectedVideoId"]);
    SetNullableString(diagnostics, "selectedSubtitleId", values[@"selectedSubtitleId"]);
    SetNullableNumber(diagnostics, "subtitleScale", values[@"subtitleScale"]);
    SetNullableNumber(diagnostics, "subtitlePosition", values[@"subtitlePosition"]);
    SetNullableNumber(diagnostics, "subtitleDelay", values[@"subtitleDelay"]);
    SetNullableNumber(diagnostics, "speed", values[@"speed"]);
    SetNullableNumber(diagnostics, "videoWidth", values[@"videoWidth"]);
    SetNullableNumber(diagnostics, "videoHeight", values[@"videoHeight"]);
    SetNullableNumber(diagnostics, "sourceFps", values[@"sourceFps"]);
    SetNullableNumber(diagnostics, "displayFps", values[@"displayFps"]);
    SetNullableNumber(diagnostics, "frameDropCount", values[@"frameDropCount"]);
    SetNullableNumber(diagnostics, "decoderFrameDropCount", values[@"decoderFrameDropCount"]);
    SetNullableNumber(diagnostics, "mistimedFrameCount", values[@"mistimedFrameCount"]);
    SetNullableNumber(diagnostics, "delayedFrameCount", values[@"delayedFrameCount"]);
    diagnostics.Set("endSequence", Napi::Number::New(info.Env(), [values[@"endSequence"] unsignedLongLongValue]));
    SetNullableString(diagnostics, "endReason", values[@"endReason"]);
    SetNullableString(diagnostics, "endError", values[@"endError"]);
    NSArray<NSDictionary<NSString *, id> *> *trackValues = values[@"tracks"];
    Napi::Array tracks = Napi::Array::New(info.Env(), trackValues.count);
    for (NSUInteger index = 0; index < trackValues.count; index += 1) {
      NSDictionary<NSString *, id> *trackValue = trackValues[index];
      Napi::Object track = Napi::Object::New(info.Env());
      id trackId = trackValue[@"id"];
      track.Set("id", trackId == [NSNull null] ? info.Env().Null() : Napi::Number::New(info.Env(), [trackId longLongValue]));
      SetNullableString(track, "type", trackValue[@"type"]);
      SetNullableString(track, "language", trackValue[@"language"]);
      SetNullableString(track, "title", trackValue[@"title"]);
      track.Set("selected", [trackValue[@"selected"] boolValue]);
      track.Set("forced", [trackValue[@"forced"] boolValue]);
      track.Set("hearingImpaired", [trackValue[@"hearingImpaired"] boolValue]);
      SetNullableString(track, "codec", trackValue[@"codec"]);
      tracks.Set(index, track);
    }
    diagnostics.Set("tracks", tracks);
    for (const char *key : { "ownedEntryId", "loadGeneration", "restartSequence", "seekGeneration", "restartedSeekGeneration", "presentedSeekGeneration" })
      SetNullableNumber(diagnostics, key, values[[NSString stringWithUTF8String:key]]);
    for (const char *key : { "fileStarted", "fileLoaded", "decodedReady", "presented", "moving" })
      diagnostics.Set(key, [values[[NSString stringWithUTF8String:key]] boolValue]);
    SetNullableString(diagnostics, "presentationEvidence", values[@"presentationEvidence"]);
    diagnostics.Set("renderReady", [values[@"renderReady"] boolValue]);
    SetNullableNumber(diagnostics, "renderUpdates", values[@"renderUpdates"]);
    diagnostics.Set("renderedFrames", Napi::Number::New(info.Env(), [values[@"renderedFrames"] unsignedLongLongValue]));
    diagnostics.Set("reportedSwaps", Napi::Number::New(info.Env(), [values[@"reportedSwaps"] unsignedLongLongValue]));
    return diagnostics;
  }

  void Destroy(const Napi::CallbackInfo &) {
    if (!view_) return;
    subtitle_bridge_.reset();
    [view_ shutdown];
    view_ = nil;
  }

  bool EnsureActive(Napi::Env env) {
    if (view_) return true;
    Napi::Error::New(env, "Native MPV host is destroyed.").ThrowAsJavaScriptException();
    return false;
  }

  static void SetNullableString(Napi::Object &target, const char *name, id value) {
    Napi::Env env = target.Env();
    if (value == [NSNull null] || ![value isKindOfClass:[NSString class]]) {
      target.Set(name, env.Null());
      return;
    }
    target.Set(name, Napi::String::New(env, [static_cast<NSString *>(value) UTF8String]));
  }

  static void SetNullableNumber(Napi::Object &target, const char *name, id value) {
    Napi::Env env = target.Env();
    target.Set(name, value == [NSNull null] ? env.Null() : Napi::Number::New(env, [value doubleValue]));
  }
};

Napi::Object Initialize(Napi::Env env, Napi::Object exports) {
  return NativeMpvHost::Init(env, exports);
}

NODE_API_MODULE(mpv_host, Initialize)
