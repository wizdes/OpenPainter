// M53: the foreground mask of an image from Apple Vision
// (VNGenerateForegroundInstanceMaskRequest, macOS 14+). See OPForegroundMask.m.
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// Error domain of `-[OPForegroundMaskJob maskForImage:error:]`.
extern NSErrorDomain const OPForegroundMaskErrorDomain;

typedef NS_ERROR_ENUM(OPForegroundMaskErrorDomain, OPForegroundMaskError) {
    /// `-cancel` was called.
    OPForegroundMaskErrorCancelled = 1,
    /// Vision failed; the underlying error is in NSUnderlyingErrorKey.
    OPForegroundMaskErrorVision = 2,
    /// The mask Vision gave has an unexpected size or format.
    OPForegroundMaskErrorBadMask = 3,
};

/// One mask computation. `-cancel` may be called from any thread.
@interface OPForegroundMaskJob : NSObject

@property (readonly, getter=isCancelled) BOOL cancelled;

/// Stops the running Vision request, if any. The run then fails with
/// OPForegroundMaskErrorCancelled.
- (void)cancel;

/// The foreground mask of `image` at the image's own size: width x height
/// bytes, rows top first, 0 (background) to 255 (foreground), soft edges
/// kept. Empty data when Vision finds no foreground. Blocks; call it off the
/// main thread.
- (nullable NSData *)maskForImage:(CGImageRef)image error:(NSError **)error;

@end

NS_ASSUME_NONNULL_END
