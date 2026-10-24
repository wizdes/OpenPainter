/* Build order: cargo build -p op-ffi --release before swift build.
 * Callbacks must copy event payloads and must not call op_*; re-entry returns OP_ERR_BUSY.
 * Inputs are borrowed for the call. Free outputs with op_buffer_free or op_pixels_free. */

#ifndef OPENPAINTER_H
#define OPENPAINTER_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

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

typedef struct OpViewState {
  double zoom;
} OpViewState;

typedef struct OpPointerEvent {
  double x;
  double y;
  float pressure;
  uint32_t phase;
} OpPointerEvent;

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

enum OpStatus op_view_attach(struct OpEngine *_p, uint64_t _doc, void *_layer, uint64_t *_out);

enum OpStatus op_view_set(struct OpEngine *_p, uint64_t _view, const struct OpViewState *_state);

enum OpStatus op_view_detach(struct OpEngine *_p, uint64_t _view);

enum OpStatus op_pointer(struct OpEngine *_p,
                         uint64_t _view,
                         const struct OpPointerEvent *_events,
                         uintptr_t _len);

#endif  /* OPENPAINTER_H */
