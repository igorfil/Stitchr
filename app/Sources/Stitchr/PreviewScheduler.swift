@MainActor
final class PreviewScheduler {
    private var task: Task<Void, Never>?

    @discardableResult
    func schedule(_ operation: @escaping @MainActor @Sendable () async -> Void) -> Task<Void, Never> {
        let previousTask = task
        previousTask?.cancel()
        let nextTask = Task {
            // Cancellation cannot interrupt a synchronous Rust render.
            await previousTask?.value
            guard !Task.isCancelled else { return }
            await operation()
        }
        task = nextTask
        return nextTask
    }

    func cancel() {
        task?.cancel()
    }
}
