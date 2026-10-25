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

    struct ReadError: Error { let code: kern_return_t }

    /// The process's physical footprint in bytes; throws when it cannot be read.
    static func footprint() throws -> UInt64 {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        guard kr == KERN_SUCCESS else { throw ReadError(code: kr) }
        return info.phys_footprint
    }

    /// Samples the footprint every 10 ms on its own thread while a fuzz test
    /// runs, so memory that rises and falls inside one input is caught too.
    final class MemoryWatch: @unchecked Sendable {
        private let lock = NSLock()
        private var running = true
        private(set) var baseline: UInt64 = 0
        private var peak: UInt64 = 0
        private var error: (any Error)?

        init() throws {
            baseline = try FuzzLimits.footprint()
            peak = baseline
            let thread = Thread { [weak self] in
                while let self, self.sample() {
                    Thread.sleep(forTimeInterval: 0.01)
                }
            }
            thread.start()
        }

        private func sample() -> Bool {
            lock.lock()
            defer { lock.unlock() }
            guard running else { return false }
            do {
                peak = max(peak, try FuzzLimits.footprint())
            } catch {
                self.error = error
                running = false
            }
            return running
        }

        /// The largest growth over the baseline so far; throws a read error.
        func grown() throws -> UInt64 {
            lock.lock()
            defer { lock.unlock() }
            if let error { throw error }
            return peak &- baseline
        }

        func stop() {
            lock.lock()
            running = false
            lock.unlock()
        }
    }

    /// Saves a failing input for a later look; returns its path.
    static func save(_ input: Data, _ name: String) -> String {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("op-fuzz-\(name)-\(UUID().uuidString).bin")
        try? input.write(to: url)
        return url.path
    }
}
