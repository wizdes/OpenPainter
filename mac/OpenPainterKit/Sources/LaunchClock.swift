// Cold launch time: printed once as "OP_READY <ms>" for scripts/launch-time.sh.
import Darwin
import Foundation

public enum LaunchClock {
    nonisolated(unsafe) private static var printed = false

    /// Milliseconds since the kernel started this process (includes dyld and
    /// everything before main).
    public static func millisecondsSinceProcessStart() -> Double {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0 else {
            FileHandle.standardError.write(Data("OpenPainter: sysctl(KERN_PROC_PID) failed: \(errno)\n".utf8))
            return -1
        }
        let start = info.kp_proc.p_un.__p_starttime
        var now = timeval()
        gettimeofday(&now, nil)
        return Double(now.tv_sec - start.tv_sec) * 1000 + Double(now.tv_usec - start.tv_usec) / 1000
    }

    /// Prints "OP_READY <ms>" the first time it is called. Main thread only.
    public static func markReady() {
        guard !printed else { return }
        printed = true
        let ms = millisecondsSinceProcessStart()
        FileHandle.standardOutput.write(Data(String(format: "OP_READY %.1f\n", ms).utf8))
    }
}
