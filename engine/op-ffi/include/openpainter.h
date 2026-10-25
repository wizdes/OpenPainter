/* Build order: cargo build -p op-ffi --release before swift build.
 * Callbacks must copy event payloads and must not call op_*; re-entry returns OP_ERR_BUSY.
 * Inputs are borrowed for the call. Free outputs with op_buffer_free or op_pixels_free. */

#ifndef OPENPAINTER_H
#define OPENPAINTER_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * `OpViewState::flags`: draw the pixel grid (shown only at 400% and above).
 */
#define OP_VIEW_PIXEL_GRID 1

/**
 * `OpViewState::flags`: the Mac is going to sleep. The engine drops the view's
 * drawable surface and refuses frames until a state without this flag arrives.
 */
#define OP_VIEW_SUSPENDED 2

typedef enum OpStatus {
  OP_OK = 0,
  OP_ERR_BAD_ARGUMENT = 1,
  OP_ERR_NO_SUCH_DOC = 2,
  OP_ERR_READ_ONLY = 3,
  OP_ERR_PANIC = 4,
  OP_ERR_BUSY = 5,
  OP_ERR_INTERNAL = 6,
  OP_ERR_UNSUPPORTED = 7,
  /**
   * The job was cancelled, or its result was stale and dropped. Nothing committed.
   */
  OP_ERR_CANCELLED = 8,
} OpStatus;

/**
 * Values of `OpEvent::kind`.
 */
typedef enum OpEventKind {
  OP_EVENT_DOC_CHANGED = 1,
  OP_EVENT_HISTORY_CHANGED = 2,
  OP_EVENT_LAYERS_CHANGED = 3,
  /**
   * Exactly one per ticket. Payload: `{"status":..., "code":<OpStatus>, "error"?:...}`.
   * status is "ok", "cancelled", "dropped" (a stale preview render; code
   * OP_ERR_CANCELLED) or an error name such as "read_only" or "panic".
   */
  OP_EVENT_JOB_FINISHED = 4,
  OP_EVENT_PROGRESS = 5,
} OpEventKind;

typedef struct OpEngine OpEngine;

typedef struct OpConfig {
  uint32_t reserved;
} OpConfig;

/**
 * One engine event. `json_ptr` is valid only during the callback; copy it.
 */
typedef struct OpEvent {
  uint32_t kind;
  uint64_t doc;
  uint64_t ticket;
  const uint8_t *json_ptr;
  uintptr_t json_len;
} OpEvent;

/**
 * Runs on one engine thread, never re-entrantly. It must not call op_*; such a
 * call returns OP_ERR_BUSY (op_engine_new returns NULL, the free calls do nothing).
 */
typedef void (*OpEventFn)(const struct OpEvent*, void*);

typedef struct OpBuffer {
  uint8_t *ptr;
  uintptr_t len;
} OpBuffer;

typedef struct OpPixelBuffer {
  uint8_t *ptr;
  uintptr_t len;
  uint32_t width;
  uint32_t height;
} OpPixelBuffer;

typedef struct OpPointerEvent {
  double x;
  double y;
  float pressure;
  uint32_t phase;
} OpPointerEvent;

/**
 * One pointer sample in document pixels. `pressure` is 0..1 (1 for a mouse);
 * `tilt_x` and `tilt_y` are -1..1 (0 for a mouse); `time_ms` must not go back.
 */
typedef struct OpStrokeSample {
  double x;
  double y;
  double pressure;
  double tilt_x;
  double tilt_y;
  uint64_t time_ms;
} OpStrokeSample;

/**
 * Where and how a view shows its document. Sizes and positions are in points;
 * the drawable is `width_pt * scale` by `height_pt * scale` pixels.
 */
typedef struct OpViewState {
  double width_pt;
  double height_pt;
  /**
   * Backing scale factor of the window's screen (1 or 2 on current Macs).
   */
  double scale;
  /**
   * Points per document pixel: 1.0 is 100%. Allowed range 0.001 to 64.
   */
  double zoom;
  /**
   * Position of the document's top-left corner, in points from the view's
   * top-left corner (y grows down).
   */
  double origin_x;
  double origin_y;
  /**
   * ICC bytes of the window screen's color space, borrowed for the call.
   * Length 0 keeps the profile sent before. Until one is sent, no display
   * conversion happens and the layer keeps whatever tag it has.
   */
  const uint8_t *display_icc;
  uintptr_t display_icc_len;
  /**
   * `OP_VIEW_PIXEL_GRID` | `OP_VIEW_SUSPENDED`.
   */
  uint32_t flags;
} OpViewState;

/**
 * What a view holds now (for tests and diagnostics).
 */
typedef struct OpViewInfo {
  uint64_t doc;
  double width_pt;
  double height_pt;
  double scale;
  double zoom;
  double origin_x;
  double origin_y;
  uint32_t flags;
  /**
   * 1 when the layer's colorspace equals the display ICC sent last.
   */
  uint8_t layer_tag_is_display;
  /**
   * 1 when a document -> display table is in use.
   */
  uint8_t has_display_table;
  uint8_t suspended;
  uint8_t _pad;
  uint64_t display_icc_len;
  /**
   * Frames presented since attach.
   */
  uint64_t frames;
  uint32_t drawable_width;
  uint32_t drawable_height;
  uint64_t configures;
  uint64_t tag_changed_by_configure;
  uint64_t retags;
  uint64_t table_builds;
  uint64_t mip_builds;
  uint64_t doc_uploads;
  uint64_t doc_revision;
  /**
   * Frames that failed (surface, color or upload errors, panics).
   */
  uint64_t frame_errors;
  /**
   * OpStatus of the last failed frame, or OP_OK.
   */
  uint32_t last_error;
  /**
   * Changes uploaded as changed tiles only, not the whole image (M38).
   */
  uint64_t region_uploads;
  /**
   * Tiles uploaded by those partial uploads.
   */
  uint64_t region_tiles;
  /**
   * The fill around the document, red, green, blue in 0..=1 (M56).
   */
  float background_red;
  float background_green;
  float background_blue;
  float _pad2;
} OpViewInfo;

/**
 * Engine-wide view counters, for leak checks. All zero before the first attach.
 */
typedef struct OpViewCounters {
  uint64_t live_views;
  /**
   * CAMetalLayers the engine retains.
   */
  uint64_t live_layers;
  uint64_t live_surfaces;
  uint64_t wgpu_textures;
  uint64_t wgpu_texture_views;
  uint64_t wgpu_buffers;
  uint64_t wgpu_bind_groups;
  uint64_t validation_errors;
  uint64_t frames_total;
} OpViewCounters;

/**
 * One presented frame, for latency measurements (M38). `time_ns` is
 * CLOCK_UPTIME_RAW (the clock of `NSEvent.timestamp` and
 * `ProcessInfo.systemUptime`) after the GPU finished the frame.
 */
typedef struct OpFrameStamp {
  uint64_t time_ns;
  /**
   * The document revision the frame shows.
   */
  uint64_t revision;
  /**
   * The preview (`stroke.append` returns it) the frame shows; 0 for none.
   */
  uint64_t preview_serial;
} OpFrameStamp;

struct OpEngine *op_engine_new(const struct OpConfig *_config, OpEventFn event_fn, void *user);

/**
 * Closes every document (queued operations and in-flight jobs finish first),
 * stops all engine threads and delivers the last events. No callback runs after
 * this returns.
 */
void op_engine_free(struct OpEngine *engine);

enum OpStatus op_import_pixels(struct OpEngine *p,
                               uint32_t width,
                               uint32_t height,
                               const uint8_t *rgba,
                               uintptr_t len,
                               const uint8_t *icc,
                               uintptr_t icc_len,
                               uint64_t *out_doc);

enum OpStatus op_doc_open_png(struct OpEngine *p,
                              const uint8_t *path,
                              uintptr_t len,
                              uint64_t *out_doc);

/**
 * Queues a command on the document and returns at once with a ticket. Commands
 * on one document run in call order. `cmd_id` is a command id, or one of
 * "preview.begin" ({"command":id,"params":{...}}), "preview.update" (params),
 * "preview.commit", "preview.cancel". A bad id or bad parameters return
 * OP_ERR_BAD_ARGUMENT now; other failures arrive as the ticket's JobFinished.
 */
enum OpStatus op_dispatch(struct OpEngine *p,
                          uint64_t doc,
                          const char *cmd_id,
                          const uint8_t *json,
                          uintptr_t len,
                          uint64_t *out_ticket);

enum OpStatus op_query(struct OpEngine *p,
                       uint64_t doc,
                       const char *query_id,
                       const uint8_t *json,
                       uintptr_t len,
                       struct OpBuffer *out);

/**
 * Cancels a ticket. Queued work is skipped; running work stops at its next check
 * and reports JobFinished with status "cancelled". Nothing it did is committed.
 */
enum OpStatus op_cancel(struct OpEngine *p, uint64_t ticket);

/**
 * Waits for the document's queued operations and in-flight jobs, then frees it.
 */
enum OpStatus op_doc_close(struct OpEngine *p, uint64_t doc);

void op_buffer_free(struct OpBuffer *b);

void op_pixels_free(struct OpPixelBuffer *b);

enum OpStatus op_pointer(struct OpEngine *_p,
                         uint64_t _view,
                         const struct OpPointerEvent *_events,
                         uintptr_t _len);

/**
 * File > New: a one-layer image named "Background" filled with
 * `background_rgba` (0xRRGGBBAA, straight alpha; 0 is Transparent) at `dpi`
 * pixels per inch. The new image is not modified.
 */
enum OpStatus op_doc_new(struct OpEngine *p,
                         uint32_t width,
                         uint32_t height,
                         double dpi,
                         uint32_t background_rgba,
                         uint64_t *out_doc);

/**
 * Opens a `.opaint` file (UTF-8 path). A damaged file returns
 * OP_ERR_BAD_ARGUMENT; a file that cannot be read returns OP_ERR_INTERNAL.
 */
enum OpStatus op_doc_open_opaint(struct OpEngine *p,
                                 const uint8_t *path,
                                 uintptr_t len,
                                 uint64_t *out_doc);

/**
 * Saves the document's current history step. `json`:
 * `{"path":"/x.opaint","format":"opaint"|"png","mark_saved":true}`.
 * `mark_saved` (default true) makes that step the saved one, so `doc.info`
 * reports `dirty: false` until the history moves off it; Save a Copy passes
 * false. The file is replaced only after it is fully written.
 */
enum OpStatus op_doc_save(struct OpEngine *p, uint64_t doc, const uint8_t *json, uintptr_t len);

/**
 * Marks the current step saved after the app wrote the file itself.
 * `revision` is the `doc.info` revision the pixels were read at; when the
 * document changed since, this returns OP_ERR_BUSY and the mark stays.
 */
enum OpStatus op_doc_mark_saved(struct OpEngine *p, uint64_t doc, uint64_t revision);

/**
 * Revert: `doc` takes `source`'s content as a fresh, saved, one-step
 * history. The document id, its views and its window stay; `source` stays
 * open for the caller to close.
 */
enum OpStatus op_doc_replace(struct OpEngine *p, uint64_t doc, uint64_t source);

/**
 * Encodes `rgba` (`width` × `height` straight RGBA8, top row first) into
 * `out` (free with op_buffer_free). `json` holds the options, such as
 * `{"format":"jpeg","jpeg_quality":95,"jpeg_subsampling":"422","dpi":300}`;
 * see op_engine::save_export. `icc`, `exif` (a raw TIFF-structure EXIF block)
 * and `xmp` (an XMP packet) may be empty. Needs no engine or document.
 */
enum OpStatus op_encode_image(const uint8_t *json,
                              uintptr_t json_len,
                              uint32_t width,
                              uint32_t height,
                              const uint8_t *rgba,
                              uintptr_t rgba_len,
                              const uint8_t *icc,
                              uintptr_t icc_len,
                              const uint8_t *exif,
                              uintptr_t exif_len,
                              const uint8_t *xmp,
                              uintptr_t xmp_len,
                              struct OpBuffer *out);

/**
 * `op_import_pixels` at `dpi` pixels per inch (1 to 100000).
 */
enum OpStatus op_import_pixels_at(struct OpEngine *p,
                                  uint32_t width,
                                  uint32_t height,
                                  const uint8_t *rgba,
                                  uintptr_t len,
                                  const uint8_t *icc,
                                  uintptr_t icc_len,
                                  double dpi,
                                  uint64_t *out_doc);

/**
 * Starts a Paintbrush, Eraser or Pencil stroke on the active layer. `json`:
 * `{"tool","width","color":"rrggbbaa","opacity":0..255,"antialias","blend",
 * "hardness":0..1,"pressure_size","pressure_opacity"}`. Returns OP_ERR_BUSY
 * while another stroke, a preview or a move runs on the document, and
 * OP_ERR_BAD_ARGUMENT for bad options.
 */
enum OpStatus op_stroke_begin(struct OpEngine *p, uint64_t doc, const uint8_t *json, uintptr_t len);

/**
 * Adds samples to the document's stroke and publishes the new preview; the
 * document's canvas views draw it at once. Writes the preview's serial (the
 * `preview_serial` of the frame stamps that show it) to `out_serial` when it
 * is not null. A bad sample changes nothing and returns OP_ERR_BAD_ARGUMENT.
 */
enum OpStatus op_stroke_append(struct OpEngine *p,
                               uint64_t doc,
                               const struct OpStrokeSample *samples,
                               uintptr_t count,
                               uint64_t *out_serial);

/**
 * Commits the document's stroke as one history step named after the tool.
 * Returns OP_ERR_BUSY (and drops the stroke) when the document changed under it.
 */
enum OpStatus op_stroke_end(struct OpEngine *p, uint64_t doc);

/**
 * Drops the document's stroke; the document and its history stay as they were.
 * Returns OP_ERR_BAD_ARGUMENT when no stroke runs.
 */
enum OpStatus op_stroke_cancel(struct OpEngine *p, uint64_t doc);

/**
 * Attaches a view of `doc` to a `CAMetalLayer`. The engine retains the layer
 * until the view is detached. Nothing is drawn until the first `op_view_set`.
 * Returns OP_ERR_NO_SUCH_DOC for an unknown document, OP_ERR_BAD_ARGUMENT for a
 * null pointer or an object that is not a CAMetalLayer, OP_ERR_INTERNAL when no
 * GPU device can be made, and OP_ERR_UNSUPPORTED off macOS.
 */
enum OpStatus op_view_attach(struct OpEngine *p, uint64_t doc, void *layer, uint64_t *out_view);

/**
 * Stores the view's newest state and asks for a frame. Returns at once; the
 * render thread draws the newest state it finds. A bad field returns
 * OP_ERR_BAD_ARGUMENT and changes nothing. A stale view id returns
 * OP_ERR_NO_SUCH_DOC.
 */
enum OpStatus op_view_set(struct OpEngine *p, uint64_t view, const struct OpViewState *state);

/**
 * Sets the color drawn around the document (the canvas surround, M56: the
 * app sends its `stage` appearance token). Components are 0 to 1 in the
 * display's encoding, as the default mid grey is. Until it is called the
 * view uses that grey. A component outside 0..=1 (or NaN) returns
 * OP_ERR_BAD_ARGUMENT and changes nothing; a stale view id returns
 * OP_ERR_NO_SUCH_DOC. Returns at once and asks for a frame.
 */
enum OpStatus op_view_set_background(struct OpEngine *p,
                                     uint64_t view,
                                     double red,
                                     double green,
                                     double blue);

/**
 * Detaches a view: waits for its in-flight frame, drops its surface and
 * releases the layer. A stale id returns OP_ERR_NO_SUCH_DOC.
 */
enum OpStatus op_view_detach(struct OpEngine *p, uint64_t view);

/**
 * Reads a view's current state and statistics.
 */
enum OpStatus op_view_info(struct OpEngine *p, uint64_t view, struct OpViewInfo *out);

/**
 * Reads the engine-wide view counters (live views, retained layers, surfaces,
 * wgpu objects, validation errors, frames).
 */
enum OpStatus op_view_counters(struct OpEngine *p, struct OpViewCounters *out);

/**
 * Moves up to `cap` present timestamps (milliseconds since attach, oldest
 * first) out of the view's log into `out` and writes how many to `out_len`.
 * The log keeps the newest 16384 frames until read.
 */
enum OpStatus op_view_take_frame_times(struct OpEngine *p,
                                       uint64_t view,
                                       double *out,
                                       uintptr_t cap,
                                       uintptr_t *out_len);

/**
 * Waits until the view has presented at least `min_frames` frames, or until
 * `timeout_ms` passes (then OP_ERR_BUSY). Writes the frame count to `out_frames`
 * when it is not null. Returns OP_ERR_NO_SUCH_DOC if the view is detached.
 */
enum OpStatus op_view_wait_frames(struct OpEngine *p,
                                  uint64_t view,
                                  uint64_t min_frames,
                                  uint32_t timeout_ms,
                                  uint64_t *out_frames);

/**
 * Moves up to `cap` frame stamps (oldest first) out of the view's log into
 * `out` and writes how many to `out_len`. The log keeps the newest 16384
 * frames until read. Separate from `op_view_take_frame_times`: reading one
 * does not drain the other.
 */
enum OpStatus op_view_take_frame_stamps(struct OpEngine *p,
                                        uint64_t view,
                                        struct OpFrameStamp *out,
                                        uintptr_t cap,
                                        uintptr_t *out_len);

#endif  /* OPENPAINTER_H */
