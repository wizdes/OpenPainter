using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;

namespace SpikeWinUI;

/// <summary>
/// A stand-in for ISwapChainPanelNative. wgpu calls SetSwapChain from the engine's render thread,
/// and a XAML panel takes that call only on the UI thread, so this carries the one call across.
/// It lives for the whole process, which is enough for this test.
/// </summary>
internal static unsafe class PanelBridge {
    static nint realPanel;
    static DispatcherQueue? queue;
    static nint* table;
    static nint* self;

    public static nint Create(nint panelNative, DispatcherQueue uiQueue) {
        realPanel = panelNative;
        queue = uiQueue;
        table = (nint*)NativeMemory.Alloc((nuint)(4 * sizeof(nint)));
        table[0] = (nint)(delegate* unmanaged[Stdcall]<nint, Guid*, nint*, int>)&QueryInterface;
        table[1] = (nint)(delegate* unmanaged[Stdcall]<nint, uint>)&AddRef;
        table[2] = (nint)(delegate* unmanaged[Stdcall]<nint, uint>)&Release;
        table[3] = (nint)(delegate* unmanaged[Stdcall]<nint, nint, int>)&SetSwapChain;
        self = (nint*)NativeMemory.Alloc((nuint)sizeof(nint));
        *self = (nint)table;
        return (nint)self;
    }

    /// <summary>Call on the UI thread before the engine frees its device: the panel must not keep drawing a dead swap chain.</summary>
    public static void Clear() {
        if (realPanel == 0) return;
        var slot = (*(nint**)realPanel)[3];
        Marshal.ThrowExceptionForHR(((delegate* unmanaged[Stdcall]<nint, nint, int>)slot)(realPanel, 0));
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvStdcall)])]
    static int QueryInterface(nint me, Guid* iid, nint* result) {
        if (*iid == new Guid("00000000-0000-0000-C000-000000000046") || *iid == new Guid("63AAD0B8-7C24-40FF-85A8-640D944CC325")) {
            *result = me;
            return 0;
        }
        *result = 0;
        return unchecked((int)0x80004002);
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvStdcall)])]
    static uint AddRef(nint me) => 1;

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvStdcall)])]
    static uint Release(nint me) => 1;

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvStdcall)])]
    static int SetSwapChain(nint me, nint swapChain) {
        const int Failed = unchecked((int)0x80004005);
        if (swapChain != 0) Marshal.AddRef(swapChain);
        // No wait here: the UI thread can be waiting on a view lock that the render thread holds
        // during this call, and waiting would block both. Presenting before the panel has the
        // swap chain does no harm, so a failure is printed from the UI thread.
        var posted = queue!.TryEnqueue(() => {
            var slot = (*(nint**)realPanel)[3];
            var result = ((delegate* unmanaged[Stdcall]<nint, nint, int>)slot)(realPanel, swapChain);
            if (swapChain != 0) Marshal.Release(swapChain);
            if (result != 0) Console.Error.WriteLine($"PANEL_BRIDGE: SetSwapChain returned {result:X8}");
        });
        if (posted) return 0;
        Console.Error.WriteLine("PANEL_BRIDGE: the UI queue refused the call");
        if (swapChain != 0) Marshal.Release(swapChain);
        return Failed;
    }
}
