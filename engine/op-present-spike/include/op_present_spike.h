// M09 presenter spike: a small C interface over two ways to show the canvas.
// Path A: wgpu renders into a CAMetalLayer and converts document -> display color
//         with a 33^3 table; the layer is tagged with the display profile.
// Path B: wgpu renders into a ring of 2 or 3 IOSurfaces; a Swift-owned layer
//         presents them tagged with the document profile (Core Animation converts).
// Every call runs on the caller's thread (the spike uses the main thread only).
// Every call catches Rust panics and returns OP_PS_PANIC instead.
#ifndef OP_PRESENT_SPIKE_H
#define OP_PRESENT_SPIKE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef int32_t OpPsStatus;
enum {
    OP_PS_OK = 0,
    OP_PS_NULL = 1,          // a required pointer was null
    OP_PS_INVALID = 2,       // bad argument (sizes, ICC data, ring length)
    OP_PS_GPU = 3,           // wgpu reported an error; see op_ps_last_error
    OP_PS_BUSY = 4,          // path B: every ring slot is being written or shown
    OP_PS_NO_FRAME = 5,      // path B: no finished frame is waiting
    OP_PS_SURFACE = 6,       // path A: the drawable could not be acquired
    OP_PS_SIZE_MISMATCH = 7, // path B: ring surfaces do not match the view size
    OP_PS_SUSPENDED = 8,     // the drawable pipeline is dropped (sleep)
    OP_PS_PANIC = 99,
};

typedef struct OpPsContext OpPsContext;
typedef struct OpPsPresenter OpPsPresenter;

typedef struct {
    double width_pt;
    double height_pt;
    double scale;    // backing scale; pixel size = max(1, round(pt * scale))
    double zoom;     // target pixels per document pixel
    double origin_x; // target pixel position of document pixel (0, 0)
    double origin_y;
} OpPsView;

typedef struct {
    uint32_t slot;       // path B ring slot (0 for path A)
    uint32_t width;      // pixels rendered
    uint32_t height;
    uint64_t seq;        // frame number
    uint64_t doc_generation;
    void *iosurface;     // path B: the IOSurfaceRef of this slot (borrowed)
    double acquire_ms;   // path A: time waiting for a drawable
    double render_ms;    // encode + submit + wait for the GPU to finish
} OpPsFrame;

typedef struct {
    uint64_t frames;
    uint64_t validation_errors;   // context-wide count from the uncaptured-error handler
    uint64_t configures;          // path A surface configures
    uint64_t tag_kept_by_configure;
    uint64_t tag_changed_by_configure;
    uint64_t retags;              // times the presenter had to set the layer colorspace
    uint64_t surface_errors;      // path A acquire failures (recovered or not)
    uint64_t ring_violations;     // path B state machine violations (must stay 0)
    uint64_t doc_generation;
    double last_acquire_ms;
    double last_render_ms;
    uint32_t drawable_width;      // path A: CAMetalLayer.drawableSize read back
    uint32_t drawable_height;
    uint32_t pixel_format;        // path A: CAMetalLayer.pixelFormat (MTLPixelFormat)
    uint8_t layer_tag_is_display; // path A: layer colorspace ICC == display ICC
    uint8_t framebuffer_only;     // path A: CAMetalLayer.framebufferOnly read back
    uint8_t _pad[2];
    char colorspace_name[96];     // path A: CGColorSpaceCopyName of the layer tag, "(icc)" or "(none)"
    char last_error[256];
} OpPsStats;

// Live object counts. The wgpu_* fields come from wgpu's internal hal counters
// (wgpu built with the "counters" feature).
typedef struct {
    uint64_t live_presenters;
    uint64_t live_ring_textures;
    uint64_t live_surfaces;
    uint64_t wgpu_textures;
    uint64_t wgpu_texture_views;
    uint64_t wgpu_buffers;
    uint64_t wgpu_bind_groups;
    uint64_t wgpu_render_pipelines;
    uint64_t wgpu_texture_memory;
    uint64_t validation_errors;
} OpPsReport;

OpPsStatus op_ps_context_new(OpPsContext **out);
void op_ps_context_free(OpPsContext *ctx);
// Waits for the GPU and frees resources wgpu dropped lazily.
OpPsStatus op_ps_context_maintain(OpPsContext *ctx);
OpPsStatus op_ps_context_report(OpPsContext *ctx, OpPsReport *out);

// Path A. The layer must be a CAMetalLayer; the presenter retains it.
OpPsStatus op_ps_new_layer(OpPsContext *ctx, void *ca_metal_layer, OpPsPresenter **out);
// Path B.
OpPsStatus op_ps_new_ring(OpPsContext *ctx, OpPsPresenter **out);
// Path B: 2 or 3 BGRA8 IOSurfaceRefs of the view's pixel size. Fails with
// OP_PS_BUSY while a slot is still being shown (release it first).
OpPsStatus op_ps_set_ring(OpPsPresenter *p, void *const *iosurfaces, uint32_t count);
void op_ps_free(OpPsPresenter *p);

// Straight, encoded RGBA8 pixels and the document's ICC profile (both copied).
OpPsStatus op_ps_set_document(OpPsPresenter *p, const uint8_t *rgba, size_t len,
                              uint32_t width, uint32_t height,
                              const uint8_t *icc, size_t icc_len);
// Path A: the display profile; rebuilds the table and tags the layer.
OpPsStatus op_ps_set_display_profile(OpPsPresenter *p, const uint8_t *icc, size_t len);
OpPsStatus op_ps_set_view(OpPsPresenter *p, const OpPsView *view);

// Path A: acquire, draw, present (blocks until the GPU is done).
// Path B: draw into a free slot and submit; the slot becomes ready when the GPU is done.
OpPsStatus op_ps_render(OpPsPresenter *p, OpPsFrame *out);
// Path A: when on, every frame also copies the drawable to memory (BGRA8).
OpPsStatus op_ps_set_capture(OpPsPresenter *p, bool on);
OpPsStatus op_ps_copy_capture(OpPsPresenter *p, uint8_t *out, size_t cap,
                              uint32_t *width, uint32_t *height);

// Path B: takes the newest finished frame for presenting (ready -> presenting).
// With wait, blocks until the GPU finishes the newest submitted frame.
OpPsStatus op_ps_ring_acquire(OpPsPresenter *p, bool wait, OpPsFrame *out);
// Path B: the presenter's own GPU work reading the slot has completed.
OpPsStatus op_ps_ring_release(OpPsPresenter *p, uint32_t slot);

// Sleep and wake: drop the drawable pipeline (path A: the wgpu surface;
// path B: the ring) and rebuild it (path A only; path B gets a new ring).
OpPsStatus op_ps_suspend(OpPsPresenter *p);
OpPsStatus op_ps_resume(OpPsPresenter *p);

OpPsStatus op_ps_stats(OpPsPresenter *p, OpPsStats *out);
// Copies the calling thread's last error message (NUL-terminated).
size_t op_ps_last_error(char *buf, size_t cap);
// Test hook: converts `n` RGB triples (0..1) from the document profile to the display
// profile with lcms2 directly (`exact`) and through the 33^3 table on the CPU (`lut`).
// Either output may be null. Outputs are clamped to 0..1.
OpPsStatus op_ps_debug_convert(const uint8_t *doc_icc, size_t doc_len,
                               const uint8_t *display_icc, size_t display_len,
                               const float *rgb, size_t n, float *exact, float *lut);
// Test hook: panics inside the boundary and must return OP_PS_PANIC.
OpPsStatus op_ps_debug_panic(void);

#ifdef __cplusplus
}
#endif
#endif
