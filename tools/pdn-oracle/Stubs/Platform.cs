using System;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
namespace PaintDotNet.SystemLayer {
  public static class Processor { public static int LogicalCpuCount => 1; }
  public static unsafe class Memory {
    public static IntPtr Allocate(ulong n) => Marshal.AllocHGlobal(checked((nint)n));
    public static IntPtr AllocateLarge(ulong n) => Allocate(n);
    public static IntPtr AllocateBitmap(int w,int h,out IntPtr handle) { handle=IntPtr.Zero; return Allocate(checked((ulong)w*(ulong)h*4)); }
    public static void Free(IntPtr p) => Marshal.FreeHGlobal(p);
    public static void FreeLarge(IntPtr p, ulong n) => Free(p);
    public static void FreeBitmap(IntPtr p,int w,int h) => Free(p);
    public static void SetToZero(void* p,ulong n)=>new Span<byte>(p,checked((int)n)).Clear();
    public static void ProtectBlockLarge(IntPtr p,ulong n,bool a,bool b) { }
    public static void Copy(void* dst,void* src,ulong n) => Buffer.MemoryCopy(src,dst,(long)n,(long)n);
  }
}
namespace PaintDotNet {
  public static class Tracing { public static void Ping(string message) { } }
  public sealed class PdnRegion { public Rectangle[] GetRegionScansReadOnlyInt() => throw new NotSupportedException(); }
  public static partial class PdnResources { public static string GetString(string key) => key; }
}
namespace PaintDotNet.Threading {
  public sealed class ThreadPool {
    public ThreadPool(int workers) { }
    public void QueueUserWorkItem(System.Threading.WaitCallback callback, object state) => callback(state);
    public void Drain() { }
  }
}

namespace PaintDotNet.Core { public sealed class Placeholder { } }
