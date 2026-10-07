#pragma once
#include <stdint.h>

struct panorama_surface_state {
    int width;
    int height;
    int visible;
    double fps;
    double scale;
    double headroom;
    uint64_t generation;
};

struct panorama_surface {
    uint64_t version;
    void *layer;
    void *opaque;
    struct panorama_surface_state (*snapshot)(void *opaque);
    void (*swapped)(void *opaque);
};
