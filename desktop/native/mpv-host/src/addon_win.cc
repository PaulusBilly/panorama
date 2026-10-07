#define WIN32_LEAN_AND_MEAN
#define UNICODE
#define _UNICODE
#include <windows.h>
#include <audioclient.h>
#include <mmdeviceapi.h>
#include <mmreg.h>
#include <ks.h>
#include <ksmedia.h>
#include <wrl/client.h>
#include <mpv/client.h>
#include <napi.h>
#include "subtitle-cue.h"
#include "playback-state.h"

#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace {

std::wstring Utf8ToWide(const std::string &value) {
  const int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                       static_cast<int>(value.size()), nullptr, 0);
  if (size <= 0) throw std::runtime_error("Invalid runtime directory encoding.");
  std::wstring result(static_cast<size_t>(size), L'\0');
  MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                      static_cast<int>(value.size()), result.data(), size);
  return result;
}

std::wstring CanonicalRuntimeDirectory(const std::string &value) {
  std::filesystem::path input(Utf8ToWide(value));
  if (!input.is_absolute()) throw std::runtime_error("The native runtime directory must be absolute.");
  std::error_code error;
  const auto canonical = std::filesystem::weakly_canonical(input, error);
  if (error || !std::filesystem::is_directory(canonical, error)) {
    throw std::runtime_error("The native runtime directory is unavailable.");
  }
  return canonical.wstring();
}

LRESULT CALLBACK VideoWindowProcedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
  if (message == WM_ERASEBKGND) return 1;
  return DefWindowProcW(window, message, wparam, lparam);
}

const wchar_t *RegisterVideoWindowClass() {
  static const wchar_t *className = L"PanoramaMpvVideoSurface";
  static std::once_flag once;
  static bool registered = false;
  std::call_once(once, [&] {
    WNDCLASSEXW value{};
    value.cbSize = sizeof(value);
    value.hInstance = GetModuleHandleW(nullptr);
    value.lpfnWndProc = VideoWindowProcedure;
    value.lpszClassName = className;
    value.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    value.hbrBackground = static_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
    registered = RegisterClassExW(&value) != 0 || GetLastError() == ERROR_CLASS_ALREADY_EXISTS;
  });
  if (!registered) throw std::runtime_error("Unable to register the video surface class.");
  return className;
}

template <typename T>
T ResolveSymbol(HMODULE module, const char *name) {
  FARPROC symbol = GetProcAddress(module, name);
  if (!symbol) throw std::runtime_error(std::string("Missing required libmpv symbol: ") + name);
  return reinterpret_cast<T>(symbol);
}

struct MpvApi {
  decltype(&mpv_create) create;
  decltype(&mpv_set_option_string) set_option_string;
  decltype(&mpv_initialize) initialize;
  decltype(&mpv_terminate_destroy) terminate_destroy;
  decltype(&mpv_command) command;
  decltype(&mpv_command_ret) command_ret;
  decltype(&mpv_command_async) command_async;
  decltype(&mpv_set_property) set_property;
  decltype(&mpv_set_property_string) set_property_string;
  decltype(&mpv_get_property) get_property;
  decltype(&mpv_get_property_string) get_property_string;
  decltype(&mpv_create_client) create_client;
  decltype(&mpv_destroy) destroy;
  decltype(&mpv_observe_property) observe_property;
  decltype(&mpv_free) free_value;
  decltype(&mpv_free_node_contents) free_node_contents;
  decltype(&mpv_wait_event) wait_event;
  decltype(&mpv_error_string) error_string;
  decltype(&mpv_wakeup) wakeup;
  decltype(&mpv_request_log_messages) request_log_messages;

  explicit MpvApi(HMODULE module)
      : create(ResolveSymbol<decltype(create)>(module, "mpv_create")),
        set_option_string(ResolveSymbol<decltype(set_option_string)>(module, "mpv_set_option_string")),
        initialize(ResolveSymbol<decltype(initialize)>(module, "mpv_initialize")),
        terminate_destroy(ResolveSymbol<decltype(terminate_destroy)>(module, "mpv_terminate_destroy")),
        command(ResolveSymbol<decltype(command)>(module, "mpv_command")),
        command_ret(ResolveSymbol<decltype(command_ret)>(module, "mpv_command_ret")),
        command_async(ResolveSymbol<decltype(command_async)>(module, "mpv_command_async")),
        set_property(ResolveSymbol<decltype(set_property)>(module, "mpv_set_property")),
        set_property_string(ResolveSymbol<decltype(set_property_string)>(module, "mpv_set_property_string")),
        get_property(ResolveSymbol<decltype(get_property)>(module, "mpv_get_property")),
        get_property_string(ResolveSymbol<decltype(get_property_string)>(module, "mpv_get_property_string")),
        create_client(ResolveSymbol<decltype(create_client)>(module, "mpv_create_client")),
        destroy(ResolveSymbol<decltype(destroy)>(module, "mpv_destroy")),
        observe_property(ResolveSymbol<decltype(observe_property)>(module, "mpv_observe_property")),
        free_value(ResolveSymbol<decltype(free_value)>(module, "mpv_free")),
        free_node_contents(ResolveSymbol<decltype(free_node_contents)>(module, "mpv_free_node_contents")),
        wait_event(ResolveSymbol<decltype(wait_event)>(module, "mpv_wait_event")),
        error_string(ResolveSymbol<decltype(error_string)>(module, "mpv_error_string")),
        wakeup(ResolveSymbol<decltype(wakeup)>(module, "mpv_wakeup")),
        request_log_messages(ResolveSymbol<decltype(request_log_messages)>(module, "mpv_request_log_messages")) {}
};

class NativeMpvHost : public Napi::ObjectWrap<NativeMpvHost> {
 public:
  static Napi::Object Init(Napi::Env env, Napi::Object exports) {
    const Napi::Function constructor = DefineClass(env, "NativeMpvHost", {
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
    try {
      if (info.Length() != 2 || !info[0].IsBuffer() || !info[1].IsString()) {
        throw std::runtime_error("A native window handle and runtime directory are required.");
      }
      const Napi::Buffer<uint8_t> handle = info[0].As<Napi::Buffer<uint8_t>>();
      if (handle.Length() != sizeof(uint64_t) || sizeof(void *) != sizeof(uint64_t)) {
        throw std::runtime_error("The x64 native window handle is invalid.");
      }
      std::memcpy(&parent_, handle.Data(), sizeof(parent_));
      HWND parent = parent_;
      if (!IsWindow(parent)) throw std::runtime_error("The native parent window is invalid.");

      runtime_directory_ = CanonicalRuntimeDirectory(info[1].As<Napi::String>().Utf8Value());
      const auto dll = std::filesystem::path(runtime_directory_) / L"libmpv-2.dll";
      module_ = LoadLibraryExW(dll.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
      if (!module_) throw std::runtime_error("Unable to load the packaged native playback runtime.");
      api_ = std::make_unique<MpvApi>(module_);

      video_ = CreateWindowExW(0, RegisterVideoWindowClass(), L"",
        WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
        0, 0, 1, 1, parent_, nullptr, GetModuleHandleW(nullptr), nullptr);
      if (!video_) throw std::runtime_error("Unable to create the native video surface.");
      EnableWindow(video_, FALSE);

      mpv_ = api_->create();
      if (!mpv_) throw std::runtime_error("Unable to create libmpv.");
      SetOption("terminal", "no");
      SetOption("msg-level", "all=warn");
      SetOption("keep-open", "yes");
      SetOption("hr-seek", "default");
      SetOption("vo", "gpu-next");
      SetOption("gpu-api", "d3d11");
      SetOption("gpu-context", "d3d11");
      SetOption("target-colorspace-hint", "auto");
      SetOption("hwdec", "d3d11va,auto-safe");
      SetOption("cache", "yes");
      SetOption("demuxer-max-bytes", "512MiB");
      SetOption("demuxer-max-back-bytes", "64MiB");
      SetOption("cache-pause", "yes");
      SetOption("cache-pause-wait", "2");
      SetOption("cache-on-disk", "no");
      SetOption("demuxer-cache-wait", "no");
      // Network resilience and buffering headroom for high-bitrate HTTP sources.
      // Tuning options are best-effort so an older runtime still initializes.
      TrySetOption("cache-pause-initial", "yes");
      TrySetOption("network-timeout", "60");
      SetOption("osc", "no");
      SetOption("input-default-bindings", "no");
      SetOption("input-vo-keyboard", "no");
      SetOption("audio-client-name", "Panorama");
      SetOption("wid", std::to_string(reinterpret_cast<uintptr_t>(video_)));
      if (api_->initialize(mpv_) < 0) throw std::runtime_error("Unable to initialize libmpv.");
      if (api_->observe_property(mpv_, 1, "time-pos", MPV_FORMAT_DOUBLE) < 0 ||
          api_->observe_property(mpv_, 2, "duration", MPV_FORMAT_DOUBLE) < 0) {
        throw std::runtime_error("Unable to observe the native playback clock.");
      }
      // Tracks and audio devices change rarely; observing them avoids dozens of
      // synchronous core round-trips per diagnostics poll on remuxes.
      api_->observe_property(mpv_, 3, "track-list", MPV_FORMAT_NODE);
      api_->observe_property(mpv_, 4, "audio-device-list", MPV_FORMAT_NONE);
      api_->request_log_messages(mpv_, "error");
      initialized_ = true;
      subtitle_bridge_ = std::make_unique<PanoramaSubtitleBridge>(mpv_, PanoramaSubtitleApi{ api_->create_client, api_->destroy, api_->observe_property, api_->wait_event, api_->get_property, api_->get_property_string, api_->set_property_string, api_->free_value, api_->free_node_contents, api_->wakeup });
      event_thread_ = std::thread([this] { EventLoop(); });
    } catch (const std::exception &error) {
      Shutdown();
      Napi::Error::New(info.Env(), error.what()).ThrowAsJavaScriptException();
    }
  }

  ~NativeMpvHost() override { Shutdown(); }

 private:
  HWND parent_ = nullptr;
  HWND video_ = nullptr;
  HMODULE module_ = nullptr;
  std::wstring runtime_directory_;
  std::unique_ptr<MpvApi> api_;
  mpv_handle *mpv_ = nullptr;
  std::thread event_thread_;
  std::unique_ptr<PanoramaSubtitleBridge> subtitle_bridge_;

  void OnSubtitleCue(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1) return;
    subtitle_bridge_->Listen(info.Env(), info[0]);
  }
  std::atomic<bool> stopping_{false};
  bool initialized_ = false;
  bool destroyed_ = false;
  std::mutex state_mutex_;
  uint64_t end_sequence_ = 0;
  std::atomic<uint64_t> audio_output_error_sequence_{0};
  struct AudioDevice {
    std::string name;
    std::string description;
    std::vector<std::string> codecs;
  };
  std::vector<AudioDevice> audio_devices_;
  std::atomic<bool> audio_devices_dirty_{true};
  struct Track {
    bool has_id = false;
    int64_t id = 0;
    std::string type;
    std::string language;
    std::string title;
    std::string codec;
    bool selected = false;
    bool forced = false;
    bool hearing_impaired = false;
  };
  std::vector<Track> tracks_;
  std::string end_reason_;
  std::string end_error_;
  bool has_time_seconds_ = false;
  double time_seconds_ = 0;
  bool has_duration_seconds_ = false;
  double duration_seconds_ = 0;
  double last_time_seconds_ = -1;
  PlaybackState playback_;
  uint64_t render_probe_token_ = 0;
  struct RenderPassEvidence {
    std::vector<int64_t> signature;
    bool measured = false;
  };
  RenderPassEvidence render_probe_;
  bool replacement_pending_ = false;
  int64_t gap_end_entry_ = -1;
  int gap_end_reason_ = 0;
  int gap_end_error_ = 0;
  bool current_eof_ = false;
  bool has_active_playlist_entry_ = false;
  int64_t active_playlist_entry_id_ = 0;

  void SetOption(const std::string &name, const std::string &value) {
    if (api_->set_option_string(mpv_, name.c_str(), value.c_str()) < 0) {
      throw std::runtime_error("Unable to configure native playback.");
    }
  }

  bool TrySetOption(const std::string &name, const std::string &value) {
    return api_->set_option_string(mpv_, name.c_str(), value.c_str()) >= 0;
  }

  static const mpv_node *MapValue(const mpv_node &map, const char *key) {
    if (map.format != MPV_FORMAT_NODE_MAP || !map.u.list) return nullptr;
    for (int index = 0; index < map.u.list->num; index += 1) {
      if (std::strcmp(map.u.list->keys[index], key) == 0) return &map.u.list->values[index];
    }
    return nullptr;
  }

  static std::string NodeString(const mpv_node *value) {
    return value && value->format == MPV_FORMAT_STRING && value->u.string ? value->u.string : "";
  }

  static bool NodeFlag(const mpv_node *value) {
    return value && value->format == MPV_FORMAT_FLAG && value->u.flag != 0;
  }

  void UpdateTracksLocked(const mpv_node *list) {
    tracks_.clear();
    if (!list || list->format != MPV_FORMAT_NODE_ARRAY || !list->u.list) return;
    for (int index = 0; index < list->u.list->num; index += 1) {
      const mpv_node &entry = list->u.list->values[index];
      Track track;
      const mpv_node *id = MapValue(entry, "id");
      track.has_id = id && id->format == MPV_FORMAT_INT64;
      if (track.has_id) track.id = id->u.int64;
      track.type = NodeString(MapValue(entry, "type"));
      track.language = NodeString(MapValue(entry, "lang"));
      track.title = NodeString(MapValue(entry, "title"));
      track.codec = NodeString(MapValue(entry, "codec"));
      track.selected = NodeFlag(MapValue(entry, "selected"));
      track.forced = NodeFlag(MapValue(entry, "forced"));
      track.hearing_impaired = NodeFlag(MapValue(entry, "hearing-impaired"));
      tracks_.push_back(std::move(track));
    }
  }

  bool EnsureActive(Napi::Env env) {
    if (!destroyed_ && initialized_ && mpv_) return true;
    Napi::Error::New(env, "Native MPV host is destroyed.").ThrowAsJavaScriptException();
    return false;
  }

  void EventLoop() {
    while (!stopping_.load()) {
      mpv_event *event = api_->wait_event(mpv_, 0.1);
      if (!event) continue;
      if (playback_.Event(*event)) {
        const auto token = playback_.FrameToken();
        const auto evidence = ReadRenderPasses();
        {
          std::lock_guard lock(state_mutex_);
          render_probe_token_ = token;
          render_probe_ = evidence;
        }
        const char *redraw[] = { "osd-overlay", "2147483647", "ass-events", "", nullptr };
        api_->command_async(mpv_, 0, redraw);
      }
      if (event->event_id == MPV_EVENT_LOG_MESSAGE && event->data) {
        const auto *message = static_cast<mpv_event_log_message *>(event->data);
        if (message->prefix && message->text && std::strcmp(message->prefix, "ao") == 0 &&
            std::strstr(message->text, "Failed to initialize audio driver")) audio_output_error_sequence_.fetch_add(1);
        continue;
      }
      if (event->event_id == MPV_EVENT_START_FILE && event->data) {
        const auto *startFile = static_cast<mpv_event_start_file *>(event->data);
        StartPlaybackEntry(startFile->playlist_entry_id);
        continue;
      }
      if (event->event_id == MPV_EVENT_PROPERTY_CHANGE && event->data) {
        const auto *property = static_cast<mpv_event_property *>(event->data);
        if (event->reply_userdata == 3) {
          std::lock_guard lock(state_mutex_);
          UpdateTracksLocked(property->format == MPV_FORMAT_NODE && property->data
            ? static_cast<const mpv_node *>(property->data) : nullptr);
          continue;
        }
        if (event->reply_userdata == 4) {
          audio_devices_dirty_.store(true);
          continue;
        }
        const bool available = property->format == MPV_FORMAT_DOUBLE && property->data;
        std::lock_guard lock(state_mutex_);
        if (replacement_pending_ || !has_active_playlist_entry_) continue;
        if (event->reply_userdata == 1) {
          has_time_seconds_ = available;
          if (available) {
            const double next = *static_cast<double *>(property->data);
            time_seconds_ = next;
            if (std::fabs(next - last_time_seconds_) > 0.001) {
              last_time_seconds_ = next;
            }
          }
        } else if (event->reply_userdata == 2) {
          has_duration_seconds_ = available;
          if (available) duration_seconds_ = *static_cast<double *>(property->data);
        }
        continue;
      }
      if (event->event_id != MPV_EVENT_END_FILE || !event->data) continue;
      const auto *endFile = static_cast<mpv_event_end_file *>(event->data);
      if (endFile->reason != MPV_END_FILE_REASON_EOF && endFile->reason != MPV_END_FILE_REASON_ERROR) continue;
      std::lock_guard lock(state_mutex_);
      if (replacement_pending_) {
        // ReplacePlayback reconciles this once it knows which entry it owns.
        gap_end_entry_ = endFile->playlist_entry_id;
        gap_end_reason_ = endFile->reason;
        gap_end_error_ = endFile->error;
        continue;
      }
      if (!has_active_playlist_entry_ || endFile->playlist_entry_id != active_playlist_entry_id_) continue;
      EndPlaybackEntryLocked(endFile->reason, endFile->error);
    }
  }

  void EndPlaybackEntryLocked(int reason, int error) {
    current_eof_ = reason == MPV_END_FILE_REASON_EOF;
    end_sequence_ += 1;
    end_reason_ = reason == MPV_END_FILE_REASON_EOF ? "eof" : "error";
    end_error_ = error < 0 ? api_->error_string(error) : "";
    has_active_playlist_entry_ = false;
  }

  void Shutdown() {
    if (destroyed_) return;
    destroyed_ = true;
    stopping_.store(true);
    if (mpv_ && api_) api_->wakeup(mpv_);
    if (event_thread_.joinable()) event_thread_.join();
    subtitle_bridge_.reset();
    if (mpv_ && api_) api_->terminate_destroy(mpv_);
    mpv_ = nullptr;
    initialized_ = false;
    if (video_ && IsWindow(video_)) DestroyWindow(video_);
    video_ = nullptr;
    api_.reset();
    if (module_) FreeLibrary(module_);
    module_ = nullptr;
  }

  std::string GetString(const char *name) const {
    char *value = api_->get_property_string(mpv_, name);
    if (!value) return {};
    std::string result(value);
    api_->free_value(value);
    return result;
  }

  RenderPassEvidence ReadRenderPasses() const {
    mpv_node passes{};
    RenderPassEvidence result;
    if (api_->get_property(mpv_, "vo-passes", MPV_FORMAT_NODE, &passes) < 0) return result;
    for (const char *type : { "fresh", "redraw" }) {
      const auto *frames = MapValue(passes, type);
      if (!frames || frames->format != MPV_FORMAT_NODE_ARRAY) continue;
      result.signature.push_back(frames->u.list->num);
      for (int index = 0; index < frames->u.list->num; index += 1) {
        const auto *samples = MapValue(frames->u.list->values[index], "samples");
        if (!samples || samples->format != MPV_FORMAT_NODE_ARRAY) continue;
        result.signature.push_back(samples->u.list->num);
        for (int sample = 0; sample < samples->u.list->num; sample += 1) {
          const auto &value = samples->u.list->values[sample];
          if (value.format == MPV_FORMAT_INT64) {
            result.signature.push_back(value.u.int64);
            result.measured = true;
          }
        }
      }
    }
    api_->free_node_contents(&passes);
    return result;
  }

  template <typename T>
  bool GetValue(const char *name, mpv_format format, T *target) const {
    return api_->get_property(mpv_, name, format, target) >= 0;
  }

  static void SetNullableString(Napi::Object target, const char *name, const std::string &value) {
    target.Set(name, value.empty() ? target.Env().Null() : Napi::String::New(target.Env(), value));
  }

  static void SetNullableNumber(Napi::Object target, const char *name, bool available, double value) {
    target.Set(name, available ? Napi::Number::New(target.Env(), value) : target.Env().Null());
  }

  void SetMpvNumber(Napi::Object target, const char *name, const char *property) const {
    double value = 0;
    const bool available = GetValue(property, MPV_FORMAT_DOUBLE, &value) && std::isfinite(value);
    SetNullableNumber(target, name, available, value);
  }

  void ResetPlaybackClockLocked() {
    has_time_seconds_ = false;
    time_seconds_ = 0;
    has_duration_seconds_ = false;
    duration_seconds_ = 0;
    last_time_seconds_ = -1;
  }

  bool ReplacePlayback(const char **command, Napi::Env env) {
    {
      std::lock_guard lock(state_mutex_);
      replacement_pending_ = true;
      has_active_playlist_entry_ = false;
      current_eof_ = false;
      end_reason_.clear();
      end_error_.clear();
      gap_end_entry_ = -1;
      ResetPlaybackClockLocked();
    }
    int result = 0;
    api_->set_property_string(mpv_, "cache-pause-wait", "2");
    const bool owned = playback_.Replace([&] {
      mpv_node reply{};
      result = subtitle_bridge_->Replace([&] { return api_->command_ret(mpv_, command, &reply); });
      int64_t entry = -1;
      if (result >= 0) {
        const auto *id = MapValue(reply, "playlist_entry_id");
        if (id && id->format == MPV_FORMAT_INT64) entry = id->u.int64;
      }
      api_->free_node_contents(&reply);
      return entry;
    });
    PlaybackSnapshot playback;
    double duration = 0;
    double time = 0;
    const bool hasDuration = owned && GetValue("duration", MPV_FORMAT_DOUBLE, &duration) && std::isfinite(duration);
    const bool hasTime = owned && GetValue("time-pos", MPV_FORMAT_DOUBLE, &time) && std::isfinite(time);
    {
      std::lock_guard lock(state_mutex_);
      playback = playback_.Snapshot();
      if (owned) {
        active_playlist_entry_id_ = playback.owned_entry;
        has_active_playlist_entry_ = true;
        replacement_pending_ = !playback.file_started;
        if (playback.file_started) {
          // Clock changes drained before the claim were dropped; resample once.
          has_duration_seconds_ = hasDuration;
          duration_seconds_ = hasDuration ? duration : 0;
          has_time_seconds_ = hasTime;
          time_seconds_ = hasTime ? time : 0;
        }
        if (gap_end_entry_ == playback.owned_entry) {
          replacement_pending_ = false;
          EndPlaybackEntryLocked(gap_end_reason_, gap_end_error_);
        }
        gap_end_entry_ = -1;
      } else {
        replacement_pending_ = false;
      }
    }
    if (!owned) {
      Napi::Error::New(env, "Unable to identify native media.").ThrowAsJavaScriptException();
      return false;
    }
    if (playback.decoded_ready) {
      const auto token = playback_.FrameToken();
      const auto evidence = ReadRenderPasses();
      {
        std::lock_guard lock(state_mutex_);
        render_probe_token_ = token;
        render_probe_ = evidence;
      }
      const char *redraw[] = { "osd-overlay", "2147483647", "ass-events", "", nullptr };
      api_->command_async(mpv_, 0, redraw);
    }
    return true;
  }

  void StartPlaybackEntry(int64_t playlistEntryId) {
    std::lock_guard lock(state_mutex_);
    if (!has_active_playlist_entry_ || playlistEntryId != active_playlist_entry_id_) return;
    replacement_pending_ = false;
    ResetPlaybackClockLocked();
  }

  void Load(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 2 || !info[0].IsString() || !info[1].IsNumber()) return;
    const std::string url = info[0].As<Napi::String>().Utf8Value();
    const std::string start = "start=+" + std::to_string(info[1].As<Napi::Number>().DoubleValue());
    const char *command[] = { "loadfile", url.c_str(), "replace", "-1", start.c_str(), nullptr };
    if (!ReplacePlayback(command, info.Env())) return;
    int paused = 0;
    api_->set_property(mpv_, "pause", MPV_FORMAT_FLAG, &paused);
  }

  void SetPaused(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1 || !info[0].IsBoolean()) return;
    int paused = info[0].As<Napi::Boolean>().Value() ? 1 : 0;
    api_->set_property(mpv_, "pause", MPV_FORMAT_FLAG, &paused);
  }

  void Seek(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1 || !info[0].IsNumber()) return;
    const std::string seconds = std::to_string(info[0].As<Napi::Number>().DoubleValue());
    const char *command[] = { "seek", seconds.c_str(), "absolute+exact", nullptr };
    subtitle_bridge_->Seek(info[0].As<Napi::Number>().DoubleValue());
    api_->set_property_string(mpv_, "cache-pause-wait", "2");
    playback_.Seek([&](uint64_t token) { return api_->command_async(mpv_, token, command); });
  }

  void SetBounds(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 5) return;
    for (size_t index = 0; index < 5; index += 1) if (!info[index].IsNumber()) return;
    const double scale = info[4].As<Napi::Number>().DoubleValue();
    const int x = static_cast<int>(std::lround(info[0].As<Napi::Number>().DoubleValue() * scale));
    const int y = static_cast<int>(std::lround(info[1].As<Napi::Number>().DoubleValue() * scale));
    const int width = static_cast<int>(std::lround(info[2].As<Napi::Number>().DoubleValue() * scale));
    const int height = static_cast<int>(std::lround(info[3].As<Napi::Number>().DoubleValue() * scale));
    if (width <= 0 || height <= 0) {
      ShowWindow(video_, SW_HIDE);
      return;
    }
    SetWindowPos(video_, HWND_BOTTOM, x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
  }

  void Command(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 1 || !info[0].IsArray()) return;
    const Napi::Array input = info[0].As<Napi::Array>();
    std::vector<std::string> values;
    std::vector<const char *> command;
    for (uint32_t index = 0; index < input.Length(); index += 1) {
      const Napi::Value value = input.Get(index);
      if (!value.IsString()) return;
      values.push_back(value.As<Napi::String>().Utf8Value());
    }
    for (const auto &value : values) command.push_back(value.c_str());
    command.push_back(nullptr);
    if (!values.empty() && values.front() == "sub-add") {
      api_->command_async(mpv_, 0, command.data());
      return;
    }
    if (!values.empty() && values.front() == "loadfile") {
      ReplacePlayback(command.data(), info.Env());
      return;
    }
    if (!values.empty() && values.front() == "stop") { subtitle_bridge_->Clear(); playback_.Stop(); }
    api_->command(mpv_, command.data());
  }

  void SetProperty(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env()) || info.Length() != 2 || !info[0].IsString()) return;
    const std::string name = info[0].As<Napi::String>().Utf8Value();
    std::string value;
    if (info[1].IsBoolean()) value = info[1].As<Napi::Boolean>().Value() ? "yes" : "no";
    else if (info[1].IsNumber()) value = std::to_string(info[1].As<Napi::Number>().DoubleValue());
    else if (info[1].IsString()) value = info[1].As<Napi::String>().Utf8Value();
    else if (info[1].IsNull()) value = "no";
    else return;
    if (name == "sid") subtitle_bridge_->Select(value);
    if (api_->set_property_string(mpv_, name.c_str(), value.c_str()) < 0)
      Napi::Error::New(info.Env(), "Unable to apply native playback setting.").ThrowAsJavaScriptException();
  }

  void Stop(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env())) return;
    subtitle_bridge_->Clear();
    playback_.Stop();
    const char *command[] = { "stop", nullptr };
    api_->command(mpv_, command);
  }

  std::vector<std::string> PassthroughCodecs(const std::string &name) {
    if (!name.starts_with("wasapi/")) return {};
    const HRESULT initialized = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) return {};
    std::vector<std::string> codecs;
    {
      Microsoft::WRL::ComPtr<IMMDeviceEnumerator> enumerator;
      Microsoft::WRL::ComPtr<IMMDevice> device;
      Microsoft::WRL::ComPtr<IAudioClient> client;
      std::wstring id = Utf8ToWide(name.substr(7));
      if (!id.starts_with(L"{0.0.0.00000000}.")) id = L"{0.0.0.00000000}." + id;
      if (SUCCEEDED(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_INPROC_SERVER,
          IID_PPV_ARGS(&enumerator))) && SUCCEEDED(enumerator->GetDevice(id.c_str(), &device)) &&
          SUCCEEDED(device->Activate(__uuidof(IAudioClient), CLSCTX_INPROC_SERVER, nullptr,
            reinterpret_cast<void **>(client.GetAddressOf())))) {
        struct Format { const char *codec; GUID subtype; WORD channels; DWORD rate; };
        const Format formats[] = {
          { "ac3", KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_DIGITAL, 2, 48000 },
          { "dts", KSDATAFORMAT_SUBTYPE_IEC61937_DTS, 2, 48000 },
          { "eac3", KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_DIGITAL_PLUS, 2, 192000 },
          { "dts-hd", KSDATAFORMAT_SUBTYPE_IEC61937_DTS_HD, 8, 192000 },
          { "truehd", KSDATAFORMAT_SUBTYPE_IEC61937_DOLBY_MLP, 8, 192000 },
        };
        for (const auto &candidate : formats) {
          WAVEFORMATEXTENSIBLE format{};
          format.Format.wFormatTag = WAVE_FORMAT_EXTENSIBLE;
          format.Format.nChannels = candidate.channels;
          format.Format.nSamplesPerSec = candidate.rate;
          format.Format.wBitsPerSample = 16;
          format.Format.nBlockAlign = candidate.channels * 2;
          format.Format.nAvgBytesPerSec = candidate.rate * format.Format.nBlockAlign;
          format.Format.cbSize = sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX);
          format.Samples.wValidBitsPerSample = 16;
          format.dwChannelMask = candidate.channels == 2 ? KSAUDIO_SPEAKER_STEREO : KSAUDIO_SPEAKER_7POINT1_SURROUND;
          format.SubFormat = candidate.subtype;
          if (client->IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &format.Format, nullptr) == S_OK)
            codecs.emplace_back(candidate.codec);
        }
      }
    }
    if (SUCCEEDED(initialized)) CoUninitialize();
    return codecs;
  }

  Napi::Array AudioDevices(Napi::Env env) {
    if (audio_devices_dirty_.exchange(false) || audio_devices_.empty()) {
      audio_devices_.clear();
      int64_t count = 0;
      if (GetValue("audio-device-list/count", MPV_FORMAT_INT64, &count)) {
        for (int64_t index = 0; index < count; index += 1) {
          const auto prefix = "audio-device-list/" + std::to_string(index) + "/";
          auto name = GetString((prefix + "name").c_str());
          auto description = GetString((prefix + "description").c_str());
          if (!name.empty()) audio_devices_.push_back({ name, description, PassthroughCodecs(name) });
        }
      }
    }
    auto devices = Napi::Array::New(env, audio_devices_.size());
    for (size_t index = 0; index < audio_devices_.size(); index += 1) {
      auto device = Napi::Object::New(env);
      device.Set("name", audio_devices_[index].name);
      device.Set("description", audio_devices_[index].description);
      auto codecs = Napi::Array::New(env, audio_devices_[index].codecs.size());
      for (size_t codec = 0; codec < audio_devices_[index].codecs.size(); codec += 1)
        codecs.Set(static_cast<uint32_t>(codec), audio_devices_[index].codecs[codec]);
      device.Set("supportedPassthroughCodecs", codecs);
      devices.Set(static_cast<uint32_t>(index), device);
    }
    return devices;
  }

  Napi::Value GetDiagnostics(const Napi::CallbackInfo &info) {
    if (!EnsureActive(info.Env())) return info.Env().Undefined();
    Napi::Object result = Napi::Object::New(info.Env());
    result.Set("audioDevices", AudioDevices(info.Env()));
    result.Set("audioOutputErrorSequence", Napi::Number::New(info.Env(), static_cast<double>(audio_output_error_sequence_.load())));
    result.Set("rendererBackend", "d3d11");
    result.Set("rendererFallbackReason", info.Env().Null());
    SetNullableString(result, "audioOutputFormat", GetString("audio-out-params/format"));
    SetNullableString(result, "audioOutputDriver", GetString("current-ao"));
    SetNullableString(result, "videoOutputPrimaries", GetString("video-out-params/primaries"));
    SetNullableString(result, "videoOutputTransferFunction", GetString("video-out-params/gamma"));
    SetNullableString(result, "mpvVersion", GetString("mpv-version"));
    SetNullableString(result, "videoCodec", GetString("video-codec"));
    SetNullableString(result, "hardwareDecoder", GetString("hwdec-current"));
    SetNullableString(result, "path", GetString("path"));
    SetNullableString(result, "ffmpegVersion", GetString("ffmpeg-version"));

    double volume = 0, subScale = 0, subPosition = 0;
    double subDelay = 0, speed = 0, sourceFps = 0, displayFps = 0;
    int paused = 0, seeking = 0, buffering = 0;
    int64_t width = 0, height = 0, drops = 0, decoderDrops = 0, mistimed = 0, delayed = 0;
    bool hasTime = false, hasDuration = false;
    double time = 0, duration = 0;
    {
      std::lock_guard lock(state_mutex_);
      hasTime = has_time_seconds_;
      time = time_seconds_;
      hasDuration = has_duration_seconds_;
      duration = duration_seconds_;
    }
    SetNullableNumber(result, "timeSeconds", hasTime, time);
    SetNullableNumber(result, "durationSeconds", hasDuration, duration);
    const bool has_volume = GetValue("volume", MPV_FORMAT_DOUBLE, &volume);
    SetNullableNumber(result, "volume", has_volume, static_cast<double>(volume));
    const bool has_subScale = GetValue("sub-scale", MPV_FORMAT_DOUBLE, &subScale);
    SetNullableNumber(result, "subtitleScale", has_subScale, static_cast<double>(subScale));
    const bool has_subPosition = GetValue("sub-pos", MPV_FORMAT_DOUBLE, &subPosition);
    SetNullableNumber(result, "subtitlePosition", has_subPosition, static_cast<double>(subPosition));
    const bool has_subDelay = GetValue("sub-delay", MPV_FORMAT_DOUBLE, &subDelay);
    SetNullableNumber(result, "subtitleDelay", has_subDelay, static_cast<double>(subDelay));
    const bool has_speed = GetValue("speed", MPV_FORMAT_DOUBLE, &speed);
    SetNullableNumber(result, "speed", has_speed, static_cast<double>(speed));
    SetMpvNumber(result, "cacheSeconds", "demuxer-cache-duration");
    SetMpvNumber(result, "cacheEndSeconds", "demuxer-cache-time");
    SetMpvNumber(result, "cacheBufferingPercent", "cache-buffering-state");
    mpv_node cacheState{};
    result.Set("cacheForwardBytes", info.Env().Null());
    if (GetValue("demuxer-cache-state", MPV_FORMAT_NODE, &cacheState)) {
      if (cacheState.format == MPV_FORMAT_NODE_MAP) {
        for (int index = 0; index < cacheState.u.list->num; index += 1) {
          const mpv_node &value = cacheState.u.list->values[index];
          if (std::strcmp(cacheState.u.list->keys[index], "fw-bytes") == 0 && value.format == MPV_FORMAT_INT64) {
            result.Set("cacheForwardBytes", Napi::Number::New(info.Env(), static_cast<double>(value.u.int64)));
            break;
          }
        }
      }
      api_->free_node_contents(&cacheState);
    }
    SetMpvNumber(result, "inputBytesPerSecond", "cache-speed");
    SetMpvNumber(result, "audioSampleRate", "audio-params/samplerate");
    SetMpvNumber(result, "audioOutputSampleRate", "audio-out-params/samplerate");
    SetNullableString(result, "audioCodec", GetString("audio-codec-name"));
    SetNullableString(result, "audioChannels", GetString("audio-params/channels"));
    SetNullableString(result, "audioOutputChannels", GetString("audio-out-params/channels"));
    SetNullableString(result, "videoPixelFormat", GetString("video-params/pixelformat"));
    SetNullableString(result, "videoColorPrimaries", GetString("video-params/primaries"));
    SetNullableString(result, "videoTransferFunction", GetString("video-params/gamma"));
    const bool has_sourceFps = GetValue("estimated-vf-fps", MPV_FORMAT_DOUBLE, &sourceFps);
    SetNullableNumber(result, "sourceFps", has_sourceFps, static_cast<double>(sourceFps));
    const bool has_displayFps = GetValue("display-fps", MPV_FORMAT_DOUBLE, &displayFps);
    SetNullableNumber(result, "displayFps", has_displayFps, static_cast<double>(displayFps));
    const bool has_width = GetValue("video-params/w", MPV_FORMAT_INT64, &width);
    SetNullableNumber(result, "videoWidth", has_width, static_cast<double>(width));
    const bool has_height = GetValue("video-params/h", MPV_FORMAT_INT64, &height);
    SetNullableNumber(result, "videoHeight", has_height, static_cast<double>(height));
    const bool has_drops = GetValue("frame-drop-count", MPV_FORMAT_INT64, &drops);
    SetNullableNumber(result, "frameDropCount", has_drops, static_cast<double>(drops));
    const bool has_decoderDrops = GetValue("decoder-frame-drop-count", MPV_FORMAT_INT64, &decoderDrops);
    SetNullableNumber(result, "decoderFrameDropCount", has_decoderDrops, static_cast<double>(decoderDrops));
    const bool has_mistimed = GetValue("mistimed-frame-count", MPV_FORMAT_INT64, &mistimed);
    SetNullableNumber(result, "mistimedFrameCount", has_mistimed, static_cast<double>(mistimed));
    const bool has_delayed = GetValue("vo-delayed-frame-count", MPV_FORMAT_INT64, &delayed);
    SetNullableNumber(result, "delayedFrameCount", has_delayed, static_cast<double>(delayed));
    GetValue("pause", MPV_FORMAT_FLAG, &paused);
    GetValue("seeking", MPV_FORMAT_FLAG, &seeking);
    GetValue("paused-for-cache", MPV_FORMAT_FLAG, &buffering);
    result.Set("paused", paused != 0);
    result.Set("seeking", seeking != 0);
    // keep-open holds the last frame without END_FILE, so the end of media is
    // only visible through eof-reached.
    int eofReached = 0;
    GetValue("eof-reached", MPV_FORMAT_FLAG, &eofReached);
    {
      std::lock_guard lock(state_mutex_);
      result.Set("eofReached", !replacement_pending_ &&
        (current_eof_ || (has_active_playlist_entry_ && eofReached != 0)));
    }
    result.Set("buffering", buffering != 0);
    SetNullableString(result, "selectedAudioId", GetString("aid"));
    SetNullableString(result, "selectedVideoId", GetString("vid"));
    SetNullableString(result, "selectedSubtitleId", GetString("sid"));

    std::vector<Track> trackSnapshot;
    {
      std::lock_guard lock(state_mutex_);
      trackSnapshot = tracks_;
    }
    Napi::Array tracks = Napi::Array::New(info.Env(), trackSnapshot.size());
    for (size_t index = 0; index < trackSnapshot.size(); index += 1) {
      const Track &entry = trackSnapshot[index];
      Napi::Object track = Napi::Object::New(info.Env());
      track.Set("id", entry.has_id ? Napi::Number::New(info.Env(), static_cast<double>(entry.id)) : info.Env().Null());
      SetNullableString(track, "type", entry.type);
      SetNullableString(track, "language", entry.language);
      SetNullableString(track, "title", entry.title);
      SetNullableString(track, "codec", entry.codec);
      track.Set("selected", entry.selected);
      track.Set("forced", entry.forced);
      track.Set("hearingImpaired", entry.hearing_impaired);
      tracks.Set(static_cast<uint32_t>(index), track);
    }
    result.Set("tracks", tracks);

    uint64_t endSequence = 0;
    std::string endReason;
    std::string endError;
    {
      std::lock_guard lock(state_mutex_);
      endSequence = end_sequence_;
      endReason = end_reason_;
      endError = end_error_;
    }
    result.Set("endSequence", Napi::Number::New(info.Env(), static_cast<double>(endSequence)));
    SetNullableString(result, "endReason", endReason);
    SetNullableString(result, "endError", endError);
    result.Set("renderReady", initialized_);
    result.Set("renderUpdates", info.Env().Null());
    result.Set("renderedFrames", info.Env().Null());
    uint64_t renderToken = 0;
    RenderPassEvidence renderEvidence;
    {
      std::lock_guard lock(state_mutex_);
      renderToken = render_probe_token_;
      renderEvidence = render_probe_;
    }
    if (!playback_.Snapshot().presented && renderToken && !renderEvidence.signature.empty()) {
      const auto evidence = ReadRenderPasses();
      if (evidence.measured && evidence.signature != renderEvidence.signature) playback_.Frame(renderToken);
    }
    double movementTime = NAN;
    GetValue("time-pos", MPV_FORMAT_DOUBLE, &movementTime);
    const auto playback = playback_.Sample(movementTime, paused != 0, buffering != 0, seeking != 0);
    SetNullableNumber(result, "ownedEntryId", playback.owned_entry >= 0, static_cast<double>(playback.owned_entry));
    result.Set("loadGeneration", Napi::Number::New(info.Env(), static_cast<double>(playback.load_generation)));
    result.Set("fileStarted", playback.file_started);
    result.Set("fileLoaded", playback.file_loaded);
    result.Set("decodedReady", playback.decoded_ready);
    result.Set("presented", playback.presented);
    result.Set("presentationEvidence", "gpu-render-pass");
    result.Set("moving", playback.moving);
    result.Set("restartSequence", Napi::Number::New(info.Env(), static_cast<double>(playback.restart_sequence)));
    result.Set("seekGeneration", Napi::Number::New(info.Env(), static_cast<double>(playback.seek_generation)));
    result.Set("restartedSeekGeneration", Napi::Number::New(info.Env(), static_cast<double>(playback.restarted_seek_generation)));
    result.Set("presentedSeekGeneration", Napi::Number::New(info.Env(), static_cast<double>(playback.presented_seek_generation)));
    result.Set("reportedSwaps", info.Env().Null());
    return result;
  }

  void Destroy(const Napi::CallbackInfo &) { Shutdown(); }
};

Napi::Object Initialize(Napi::Env env, Napi::Object exports) {
  return NativeMpvHost::Init(env, exports);
}

}  // namespace

NODE_API_MODULE(mpv_host, Initialize)
