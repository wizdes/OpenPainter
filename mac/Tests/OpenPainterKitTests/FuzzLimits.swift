// M57 review item 10: limits for the Swift fuzz tests. Each input runs on its
// own thread and the test waits at most `deadline` seconds for it (a thread
// cannot be killed, so a hang fails the test at once); the process's memory
// footprint (resident plus compressed, as Activity Monitor shows) must not
// grow by more than 2 GiB over what it was when the fuzz test began (other
// tests in the same process hold memory of their own).
import Darwin
import Foundation

final class FuzzBox<T>: @unchecked Sendable {
    var value: T?
}

enum FuzzLimits {
    static let memoryCap: UInt64 = 2 << 30

    /// Runs `body(input)` on a new thread. nil: it did not finish in time.
    static func run<T: Sendable>(_ input: Data, deadline: TimeInterval,
                                 _ body: @escaping @Sendable (Data) -> T) -> T? {
        let box = FuzzBox<T>()
        let done = DispatchSemaphore(value: 0)
        let thread = Thread {
            box.value = body(input)
            done.signal()
        }
        thread.stackSize = 8 << 20
        thread.start()
        return done.wait(timeout: .now() + deadline) == .success ? box.value : nil
    }

    /// The process's physical footprint in bytes.
    static func footprint() -> UInt64 {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return kr == KERN_SUCCESS ? info.phys_footprint : 0
    }

    /// Saves a failing input for a later look; returns its path.
    static func save(_ input: Data, _ name: String) -> String {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("op-fuzz-\(name)-\(UUID().uuidString).bin")
        try? input.write(to: url)
        return url.path
    }
}
