using System;

namespace System.Drawing {
    public class Image : IDisposable { public virtual void Dispose() { } }
    public sealed class Bitmap : Image {
        public int Width {get;}
        public int Height {get;}
        public Bitmap(int width,int height,int stride,Imaging.PixelFormat format,IntPtr pixels)
        { Width=width; Height=height; throw new PlatformNotSupportedException("GDI bitmap is not used by the pixel oracle"); }
        public Imaging.BitmapData LockBits(Rectangle bounds,Imaging.ImageLockMode mode,Imaging.PixelFormat format)
            =>throw new PlatformNotSupportedException("GDI bitmap is not used by the pixel oracle");
        public void UnlockBits(Imaging.BitmapData data)
            =>throw new PlatformNotSupportedException("GDI bitmap is not used by the pixel oracle");
    }
    public sealed class Graphics : IDisposable {
        public static Graphics FromImage(Image image)
            =>throw new PlatformNotSupportedException("GDI graphics is not used by the pixel oracle");
        public void Dispose() { }
    }
    public sealed class Region : IDisposable {
        public Region() => throw new PlatformNotSupportedException("GDI regions are not used by the pixel oracle");
        public void Dispose() { }
    }
}
namespace System.Drawing.Imaging {
    public enum PixelFormat { Format32bppArgb, Format32bppRgb }
    public enum ImageLockMode { ReadOnly }
    public sealed class BitmapData { public int Width {get;set;} public int Height {get;set;} public int Stride {get;set;} public IntPtr Scan0 {get;set;} }
}
namespace System.Drawing.Drawing2D { internal sealed class NamespaceAnchor { } }
namespace PaintDotNet {
    public sealed class PdnGraphicsPath { public PdnGraphicsPath()=>throw new PlatformNotSupportedException("GPC paths are excluded"); }
}
