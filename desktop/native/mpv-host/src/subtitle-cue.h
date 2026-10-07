#pragma once
#include <mpv/client.h>
#include <napi.h>
#include <atomic>
#include <cmath>
#include <cstring>
#include <memory>
#include <mutex>
#include <regex>
#include <sstream>
#include <vector>
#include <string>
#include <thread>

struct PanoramaSubtitleApi {
  decltype(&mpv_create_client) create_client;
  decltype(&mpv_destroy) destroy;
  decltype(&mpv_observe_property) observe_property;
  decltype(&mpv_wait_event) wait_event;
  decltype(&mpv_get_property) get_property;
  decltype(&mpv_get_property_string) get_property_string;
  decltype(&mpv_set_property_string) set_property_string;
  decltype(&mpv_free) free_value;
  decltype(&mpv_free_node_contents) free_node_contents;
  decltype(&mpv_wakeup) wakeup;
};

struct PanoramaSubtitleDelivery { std::atomic<bool> active{true}; std::atomic<bool> failed{false}; std::atomic<uint64_t> playback{0}, selection{0}, seek{0}; };
struct PanoramaSubtitleCue {
  uint64_t playback = 0, selection = 0, seek = 0, sequence = 0;
  std::string track, kind = "none", text;
  double start = -1, end = -1;
  std::shared_ptr<PanoramaSubtitleDelivery> delivery;
};

class PanoramaSubtitleBridge {
 public:
  PanoramaSubtitleBridge(mpv_handle *parent, PanoramaSubtitleApi api) : api_(api) {
    client_ = api_.create_client(parent, "panorama-subtitles");
    if (!client_) return;
    const char *properties[] = { "sub-text", "sub-text/ass-full", "sub-start/full", "sub-end/full", "sid", "track-list", "seeking", "sub-ass-extradata" };
    for (size_t index = 0; index < 8; index++) api_.observe_property(client_, 100 + index, properties[index], MPV_FORMAT_NONE);
    thread_ = std::thread([this] { Run(); });
  }
  ~PanoramaSubtitleBridge() { Stop(); }

  void Listen(Napi::Env env, Napi::Value value) {
    std::lock_guard lock(mutex_);
    if (delivery_) delivery_->active.store(false);
    if (callback_) { callback_.Abort(); callback_ = {}; }
    delivery_.reset();
    if (!value.IsFunction() || !client_) { if (client_) api_.set_property_string(client_, "sub-visibility", "yes"); return; }
    delivery_ = std::make_shared<PanoramaSubtitleDelivery>();
    UpdateGeneration();
    has_last_sample_ = false;
    callback_ = Napi::ThreadSafeFunction::New(env, value.As<Napi::Function>(), "Panorama subtitles", 128, 1);
    callback_.Unref(env);
    Emit(false);
  }

  template <typename Operation>
  int Replace(Operation operation) {
    std::lock_guard lock(mutex_);
    playback_++; selection_ = 0; seek_ = 0; selection_pending_ = false; seeking_ = false; expected_track_.clear(); ready_ = false; stopped_ = false; awaiting_load_ = true; expected_entry_ = -1; active_entry_ = -1; authored_ = false;
    ResetDelivery(); Emit(true);
    const int result = operation();
    if (client_) api_.get_property(client_, "playlist/0/id", MPV_FORMAT_INT64, &expected_entry_);
    return result;
  }
  void Select(const std::string &id) {
    std::lock_guard lock(mutex_);
    const bool current = ready_ && id == String("sid");
    selection_++; ready_ = false; expected_track_ = id; selection_pending_ = true; authored_ = false;
    ResetDelivery(); Emit(true);
    if (current) { selection_pending_ = false; ready_ = true; Emit(false); }
  }
  void Seek(double seconds) {
    std::lock_guard lock(mutex_);
    seek_++; ready_ = false; seeking_ = true; seek_target_ = seconds; UpdateGeneration(); Emit(true);
  }
  void Clear() {
    std::lock_guard lock(mutex_);
    selection_++; stopped_ = true; ready_ = false; UpdateGeneration(); Emit(true);
    if (client_) api_.set_property_string(client_, "sub-visibility", "yes");
  }
  void Stop() {
    if (stopping_.exchange(true)) return;
    if (client_) api_.wakeup(client_);
    if (thread_.joinable()) thread_.join();
    std::lock_guard lock(mutex_);
    if (delivery_) delivery_->active.store(false);
    if (callback_) { callback_.Abort(); callback_ = {}; }
    if (client_) { api_.set_property_string(client_, "sub-visibility", "yes"); api_.destroy(client_); client_ = nullptr; }
  }

 private:
  PanoramaSubtitleApi api_;
  mpv_handle *client_ = nullptr;
  std::mutex mutex_;
  std::thread thread_;
  std::atomic<bool> stopping_{false};
  Napi::ThreadSafeFunction callback_;
  std::shared_ptr<PanoramaSubtitleDelivery> delivery_;
  uint64_t playback_ = 0, selection_ = 0, seek_ = 0, sequence_ = 0;
  bool ready_ = false, stopped_ = true, awaiting_load_ = false, selection_pending_ = false, seeking_ = false, authored_ = false;
  int64_t expected_entry_ = -1, active_entry_ = -1;
  std::string expected_track_;
  double seek_target_ = 0;
  PanoramaSubtitleCue last_sample_;
  bool has_last_sample_ = false;

  void UpdateGeneration() { if (delivery_) { delivery_->playback.store(playback_); delivery_->selection.store(selection_); delivery_->seek.store(seek_); } }
  void ResetDelivery() { if (delivery_) delivery_->failed.store(false); UpdateGeneration(); }
  std::string String(const char *name) {
    char *value = api_.get_property_string(client_, name);
    if (!value) return {};
    std::string result(value); api_.free_value(value); return result;
  }
  double Number(const char *name) {
    double value = -1;
    return api_.get_property(client_, name, MPV_FORMAT_DOUBLE, &value) >= 0 && std::isfinite(value) ? value : -1;
  }
  static const mpv_node *Value(const mpv_node &node, const char *key) {
    if (node.format != MPV_FORMAT_NODE_MAP || !node.u.list) return nullptr;
    for (int i = 0; i < node.u.list->num; i++) if (std::strcmp(node.u.list->keys[i], key) == 0) return &node.u.list->values[i];
    return nullptr;
  }
  std::string Codec(const std::string &id) {
    mpv_node tracks{};
    std::string codec;
    if (api_.get_property(client_, "track-list", MPV_FORMAT_NODE, &tracks) < 0) return codec;
    if (tracks.format == MPV_FORMAT_NODE_ARRAY && tracks.u.list) for (int i = 0; i < tracks.u.list->num; i++) {
      const auto &track = tracks.u.list->values[i];
      const auto *track_id = Value(track, "id"); const auto *type = Value(track, "type"); const auto *format = Value(track, "codec");
      if (track_id && track_id->format == MPV_FORMAT_INT64 && std::to_string(track_id->u.int64) == id && type && type->format == MPV_FORMAT_STRING && std::strcmp(type->u.string, "sub") == 0 && format && format->format == MPV_FORMAT_STRING) codec = format->u.string;
    }
    api_.free_node_contents(&tracks); return codec;
  }
  static std::vector<std::string> Fields(const std::string &line) {
    std::vector<std::string> fields;
    std::stringstream input(line);
    std::string field;
    while (std::getline(input, field, ',')) fields.push_back(field);
    return fields;
  }
  bool AuthoredLayout(const std::string &ass) {
    std::stringstream events(ass);
    std::string line;
    while (std::getline(events, line)) {
      if (!line.starts_with("Dialogue:")) continue;
      const auto fields = Fields(line.substr(9));
      if (fields.size() < 10) return true;
      for (size_t index : { size_t(0), size_t(5), size_t(6), size_t(7) }) {
        try { if (std::stoi(fields[index]) != 0) return true; } catch (...) { return true; }
      }
      if (fields[8].find_first_not_of(" \r\t") != std::string::npos) return true;
      std::stringstream styles(String("sub-ass-extradata"));
      std::string style;
      while (std::getline(styles, style)) {
        if (!style.starts_with("Style:")) continue;
        const auto parts = Fields(style.substr(6));
        if (parts.size() < 19) continue;
        const auto nameStart = parts[0].find_first_not_of(" ");
        const auto name = nameStart == std::string::npos ? "" : parts[0].substr(nameStart);
        if (name == fields[3]) { try { if (std::stoi(parts[18]) != 2) return true; } catch (...) { return true; } }
      }
    }
    return false;
  }
  void Emit(bool clear) {
    if (!callback_ || !delivery_ || !delivery_->active.load()) return;
    auto *cue = new PanoramaSubtitleCue;
    cue->playback = playback_; cue->selection = selection_; cue->seek = seek_; cue->sequence = ++sequence_; cue->delivery = delivery_;
    if (!clear && ready_ && !stopped_) {
      cue->track = String("sid");
      if (cue->track == "no" || cue->track == "auto" || cue->track.empty()) cue->track.clear();
      if (!cue->track.empty()) {
        const auto codec = Codec(cue->track);
        if (codec == "hdmv_pgs_subtitle" || codec == "pgs" || codec == "dvd_subtitle" || codec == "vobsub" || codec == "dvb_subtitle" || codec == "xsub") cue->kind = "bitmap";
        else if (codec == "subrip" || codec == "srt" || codec == "webvtt" || codec == "text" || codec == "mov_text" || codec == "ass" || codec == "ssa") {
          const auto ass = String("sub-text/ass-full");
          static const std::regex authored(R"(\\(?:p[1-9]|pos\(|move\(|org\(|clip\(|iclip\(|an[1-9]|a[1-9]|k[fFoO]?\d|t\(|fad\(|fade\())");
          if (std::regex_search(ass, authored) || ((codec == "ass" || codec == "ssa") && AuthoredLayout(ass))) authored_ = true;
          cue->text = String("sub-text");
          if ((codec == "ass" || codec == "ssa") && ass.empty() && !cue->text.empty()) authored_ = true;
          if (cue->text.size() > 65536 || delivery_->failed.load()) authored_ = true;
          cue->kind = authored_ ? "authored" : "text";
          if (authored_) cue->text.clear();
          cue->start = Number("sub-start/full"); cue->end = Number("sub-end/full");
        } else cue->kind = "authored";
      }
    }
    if (has_last_sample_ && last_sample_.playback == cue->playback && last_sample_.selection == cue->selection && last_sample_.seek == cue->seek && last_sample_.track == cue->track && last_sample_.kind == cue->kind && last_sample_.text == cue->text && last_sample_.start == cue->start && last_sample_.end == cue->end) { delete cue; return; }
    last_sample_ = *cue; has_last_sample_ = true;
    const auto result = callback_.NonBlockingCall(cue, [](Napi::Env env, Napi::Function listener, PanoramaSubtitleCue *raw) {
      std::unique_ptr<PanoramaSubtitleCue> value(raw);
      if (!env || !value->delivery->active.load()) return;
      if (value->playback != value->delivery->playback.load() || value->selection != value->delivery->selection.load() || value->seek != value->delivery->seek.load()) return;
      auto object = Napi::Object::New(env);
      object.Set("playbackGeneration", Napi::Number::New(env, value->playback));
      object.Set("selectionGeneration", Napi::Number::New(env, value->selection));
      object.Set("seekGeneration", Napi::Number::New(env, value->seek));
      object.Set("sequence", Napi::Number::New(env, value->sequence));
      if (value->track.empty()) object.Set("trackId", env.Null());
      else object.Set("trackId", Napi::String::New(env, value->track));
      const bool failed = value->delivery->failed.load();
      object.Set("kind", failed ? "authored" : value->kind);
      object.Set("text", failed ? "" : value->text);
      if (value->start < 0) object.Set("startSeconds", env.Null());
      else object.Set("startSeconds", Napi::Number::New(env, value->start));
      if (value->end < 0) object.Set("endSeconds", env.Null());
      else object.Set("endSeconds", Napi::Number::New(env, value->end));
      listener.Call({ object });
    });
    if (result != napi_ok) { delete cue; delivery_->failed.store(true); api_.set_property_string(client_, "sub-visibility", "yes"); }
  }
  void Run() {
    while (!stopping_.load()) {
      mpv_event *event = api_.wait_event(client_, 0.05);
      if (!event || event->event_id == MPV_EVENT_NONE) continue;
      std::lock_guard lock(mutex_);
      if (stopping_.load()) break;
      if (event->event_id == MPV_EVENT_START_FILE && event->data) {
        const auto *start = static_cast<mpv_event_start_file *>(event->data);
        if (start->playlist_entry_id == expected_entry_) active_entry_ = expected_entry_;
      }
      if (event->event_id == MPV_EVENT_FILE_LOADED && active_entry_ == expected_entry_ && active_entry_ >= 0) { awaiting_load_ = false; if (selection_pending_ && String("sid") == expected_track_) selection_pending_ = false; ready_ = !selection_pending_ && !seeking_; Emit(false); }
      if (event->event_id == MPV_EVENT_PLAYBACK_RESTART && !awaiting_load_ && !stopped_) {
        if (seeking_ && std::fabs(Number("time-pos") - seek_target_) <= 0.25) seeking_ = false;
        ready_ = !seeking_ && !selection_pending_; Emit(false);
      }
      if (event->event_id == MPV_EVENT_END_FILE && event->data) {
        const auto *end = static_cast<mpv_event_end_file *>(event->data);
        if (end->playlist_entry_id == active_entry_) { ready_ = false; Emit(true); }
      }
      if (event->event_id == MPV_EVENT_PROPERTY_CHANGE && event->data) {
        const auto *property = static_cast<mpv_event_property *>(event->data);
        if (property->name && selection_pending_ && !awaiting_load_) {
          const auto track = String("sid");
          if (track == expected_track_ || (expected_track_ == "no" && track.empty())) { selection_pending_ = false; ready_ = !seeking_ && !awaiting_load_ && !stopped_; }
        }
        if (ready_) Emit(false);
      }
    }
  }
};
