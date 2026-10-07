#pragma once
#include <mpv/client.h>
#include <cmath>
#include <cstdint>
#include <mutex>

struct PlaybackSnapshot {
  int64_t owned_entry = -1;
  uint64_t load_generation = 0;
  bool file_started = false;
  bool file_loaded = false;
  bool decoded_ready = false;
  bool presented = false;
  bool moving = false;
  uint64_t restart_sequence = 0;
  uint64_t seek_generation = 0;
  uint64_t restarted_seek_generation = 0;
  uint64_t presented_seek_generation = 0;
};

class PlaybackState {
 public:
  static constexpr uint64_t seek_reply_bit = uint64_t{1} << 63;

  template <typename Command>
  bool Replace(Command command) {
    uint64_t load;
    {
      std::lock_guard lock(mutex_);
      load = state_.load_generation + 1;
      const auto seek = state_.seek_generation;
      state_ = {};
      state_.load_generation = load;
      state_.seek_generation = seek;
      state_.restarted_seek_generation = seek;
      seek_acknowledged_ = false;
      seek_started_ = false;
      seek_was_ready_ = false;
      seek_was_presented_ = false;
      seek_load_generation_ = 0;
      last_time_ = NAN;
    }
    const auto entry = command();
    std::lock_guard lock(mutex_);
    if (state_.load_generation != load) return false;
    state_.owned_entry = entry;
    if (entry >= 0 && event_entry_ == entry) {
      state_.file_started = true;
      state_.file_loaded = event_file_loaded_;
      if (event_restarted_) Restart();
    }
    return state_.owned_entry >= 0;
  }

  template <typename Command>
  int Seek(Command command) {
    std::lock_guard lock(mutex_);
    seek_was_ready_ = state_.decoded_ready;
    seek_was_presented_ = state_.presented;
    seek_load_generation_ = state_.load_generation;
    event_restarted_ = false;
    state_.seek_generation += 1;
    state_.decoded_ready = false;
    state_.presented = false;
    state_.moving = false;
    last_time_ = NAN;
    seek_acknowledged_ = false;
    seek_started_ = false;
    const int result = command(seek_reply_bit | state_.seek_generation);
    if (result < 0) CancelSeek();
    return result;
  }

  bool Event(const mpv_event &event) {
    std::lock_guard lock(mutex_);
    if (event.event_id == MPV_EVENT_START_FILE && event.data) {
      event_entry_ = static_cast<mpv_event_start_file *>(event.data)->playlist_entry_id;
      event_file_loaded_ = false;
      event_restarted_ = false;
      if (event_entry_ == state_.owned_entry && state_.owned_entry >= 0) state_.file_started = true;
    }
    if (event.event_id == MPV_EVENT_END_FILE && event.data &&
        static_cast<mpv_event_end_file *>(event.data)->playlist_entry_id == event_entry_) event_entry_ = -1;
    // mpv rejects seeks before playback is initialized; a rejected seek must not block the next restart.
    if (event.event_id == MPV_EVENT_COMMAND_REPLY &&
        event.reply_userdata == (seek_reply_bit | state_.seek_generation) && event.error < 0) CancelSeek();
    if (event_entry_ >= 0 && event.event_id == MPV_EVENT_FILE_LOADED) event_file_loaded_ = true;
    if (event_entry_ >= 0 && event_file_loaded_ && event.event_id == MPV_EVENT_PLAYBACK_RESTART &&
        state_.seek_generation == state_.restarted_seek_generation) event_restarted_ = true;
    if (!state_.file_started || event_entry_ != state_.owned_entry) return false;
    if (event.event_id == MPV_EVENT_FILE_LOADED) state_.file_loaded = true;
    // The reply is queued before this command's SEEK; older queued events cannot arm it.
    if (event.event_id == MPV_EVENT_COMMAND_REPLY &&
        event.reply_userdata == (seek_reply_bit | state_.seek_generation) && event.error >= 0 &&
        seek_load_generation_ == state_.load_generation && seek_load_generation_ != 0) seek_acknowledged_ = true;
    if (event.event_id == MPV_EVENT_SEEK && seek_acknowledged_) seek_started_ = true;
    if (event.event_id != MPV_EVENT_PLAYBACK_RESTART || !state_.file_loaded ||
        (state_.seek_generation > state_.restarted_seek_generation && !seek_started_)) return false;
    Restart();
    return true;
  }

  uint64_t FrameToken() {
    std::lock_guard lock(mutex_);
    return state_.decoded_ready ? frame_epoch_() : 0;
  }

  void Frame(uint64_t token) {
    std::lock_guard lock(mutex_);
    if (!token || !state_.decoded_ready || token != frame_epoch_()) return;
    state_.presented = true;
    state_.presented_seek_generation = state_.restarted_seek_generation;
  }

  PlaybackSnapshot Sample(double time, bool paused, bool buffering, bool seeking) {
    std::lock_guard lock(mutex_);
    state_.moving = state_.presented && !paused && !buffering && !seeking &&
      std::isfinite(time) && ((state_.moving) || (std::isfinite(last_time_) && time > last_time_ + 0.001));
    last_time_ = !paused && !buffering && !seeking ? time : NAN;
    return state_;
  }

  PlaybackSnapshot Snapshot() {
    std::lock_guard lock(mutex_);
    return state_;
  }

  void Stop() {
    std::lock_guard lock(mutex_);
    state_.owned_entry = -1;
    state_.decoded_ready = false;
    state_.presented = false;
    state_.moving = false;
  }

 private:
  void Restart() {
    state_.decoded_ready = true;
    state_.presented = false;
    state_.moving = false;
    state_.restart_sequence += 1;
    state_.restarted_seek_generation = state_.seek_generation;
    seek_load_generation_ = 0;
    last_time_ = NAN;
  }

  void CancelSeek() {
    if (seek_load_generation_ == 0 || seek_load_generation_ != state_.load_generation) return;
    state_.decoded_ready = seek_was_ready_;
    state_.presented = seek_was_presented_;
    state_.restarted_seek_generation = state_.seek_generation;
    if (seek_was_presented_) state_.presented_seek_generation = state_.seek_generation;
    seek_load_generation_ = 0;
  }

  uint64_t frame_epoch_() const { return (state_.load_generation << 32) | state_.restart_sequence; }
  std::mutex mutex_;
  PlaybackSnapshot state_;
  int64_t event_entry_ = -1;
  bool event_file_loaded_ = false;
  bool event_restarted_ = false;
  uint64_t seek_load_generation_ = 0;
  bool seek_acknowledged_ = false;
  bool seek_started_ = false;
  bool seek_was_ready_ = false;
  bool seek_was_presented_ = false;
  double last_time_ = NAN;
};
