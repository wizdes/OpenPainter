// M53: the foreground mask from Apple Vision.
//
// Vision pages used:
// - VNGenerateForegroundInstanceMaskRequest (macOS 14+):
//   https://developer.apple.com/documentation/vision/vngenerateforegroundinstancemaskrequest
//   Its result is one VNInstanceMaskObservation; `allInstances` names every
//   foreground object found (no observation, or an empty set: no foreground).
// - VNInstanceMaskObservation generateScaledMaskForImageForInstances:fromRequestHandler:error:
//   https://developer.apple.com/documentation/vision/vninstancemaskobservation/generatescaledmaskforimage(forinstances:from:)
//   It gives the mask at the input image's size as a one-channel Float32
//   pixel buffer, 0 to 1, with soft edges. This file checks the size and
//   format and rounds each value to one byte.
// - VNRequest cancel: https://developer.apple.com/documentation/vision/vnrequest/cancel()
#import "OPForegroundMask.h"
#import <CoreVideo/CoreVideo.h>
#import <Vision/Vision.h>
#include <math.h>

NSErrorDomain const OPForegroundMaskErrorDomain = @"OPForegroundMaskErrorDomain";

static NSError *OPError(OPForegroundMaskError code, NSString *text, NSError *_Nullable underlying) {
    NSMutableDictionary *info = [@{NSLocalizedDescriptionKey: text} mutableCopy];
    if (underlying) info[NSUnderlyingErrorKey] = underlying;
    return [NSError errorWithDomain:OPForegroundMaskErrorDomain code:code userInfo:info];
}

@implementation OPForegroundMaskJob {
    NSLock *_lock;
    VNRequest *_request;
    BOOL _cancelled;
}

- (instancetype)init {
    if ((self = [super init])) _lock = [NSLock new];
    return self;
}

- (BOOL)isCancelled {
    [_lock lock];
    BOOL c = _cancelled;
    [_lock unlock];
    return c;
}

- (void)cancel {
    [_lock lock];
    _cancelled = YES;
    VNRequest *r = _request;
    [_lock unlock];
    [r cancel];
}

- (nullable NSData *)maskForImage:(CGImageRef)image error:(NSError **)error {
    NSError *(^fail)(OPForegroundMaskError, NSString *, NSError *) = ^(OPForegroundMaskError c, NSString *t, NSError *u) {
        NSError *e = OPError(c, t, u);
        if (error) *error = e;
        return e;
    };
    VNGenerateForegroundInstanceMaskRequest *request = [VNGenerateForegroundInstanceMaskRequest new];
    [_lock lock];
    _request = request;
    BOOL stop = _cancelled;
    [_lock unlock];
    if (stop) { fail(OPForegroundMaskErrorCancelled, @"cancelled", nil); return nil; }

    VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithCGImage:image options:@{}];
    NSError *visionError = nil;
    BOOL ok = [handler performRequests:@[request] error:&visionError];
    if (self.isCancelled) { fail(OPForegroundMaskErrorCancelled, @"cancelled", nil); return nil; }
    if (!ok) { fail(OPForegroundMaskErrorVision, @"Vision could not run the request", visionError); return nil; }

    VNInstanceMaskObservation *observation = request.results.firstObject;
    if (observation == nil || observation.allInstances.count == 0) return [NSData data];
    CVPixelBufferRef buffer = [observation generateScaledMaskForImageForInstances:observation.allInstances
                                                                fromRequestHandler:handler
                                                                             error:&visionError];
    if (buffer == NULL) { fail(OPForegroundMaskErrorVision, @"Vision could not make the mask", visionError); return nil; }
    if (self.isCancelled) { fail(OPForegroundMaskErrorCancelled, @"cancelled", nil); return nil; }

    size_t w = CVPixelBufferGetWidth(buffer), h = CVPixelBufferGetHeight(buffer);
    if (CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_OneComponent32Float) {
        fail(OPForegroundMaskErrorBadMask, @"the mask is not one-channel Float32", nil);
        return nil;
    }
    if (w != CGImageGetWidth(image) || h != CGImageGetHeight(image)) {
        fail(OPForegroundMaskErrorBadMask,
             [NSString stringWithFormat:@"the mask is %zu x %zu, the image is %zu x %zu", w, h,
                                        CGImageGetWidth(image), CGImageGetHeight(image)], nil);
        return nil;
    }
    NSMutableData *out = [NSMutableData dataWithLength:w * h];
    uint8_t *dst = out.mutableBytes;
    CVPixelBufferLockBaseAddress(buffer, kCVPixelBufferLock_ReadOnly);
    const uint8_t *base = CVPixelBufferGetBaseAddress(buffer);
    size_t rowBytes = CVPixelBufferGetBytesPerRow(buffer);
    for (size_t y = 0; y < h; y++) {
        const float *row = (const float *)(base + y * rowBytes);
        for (size_t x = 0; x < w; x++) {
            float v = roundf(row[x] * 255.0f);
            dst[y * w + x] = (uint8_t)(v < 0 ? 0 : (v > 255 ? 255 : v));
        }
    }
    CVPixelBufferUnlockBaseAddress(buffer, kCVPixelBufferLock_ReadOnly);
    return out;
}

@end
