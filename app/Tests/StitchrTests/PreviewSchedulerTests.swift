import Testing
@testable import Stitchr

@MainActor
private final class Gate {
    private var isOpen = false
    private var continuation: CheckedContinuation<Void, Never>?

    func wait() async {
        guard !isOpen else { return }
        await withCheckedContinuation { continuation = $0 }
    }

    func open() {
        isOpen = true
        continuation?.resume()
        continuation = nil
    }
}

@MainActor
struct PreviewSchedulerTests {
    @Test(.timeLimit(.minutes(1)))
    func rendersOnlyLatestPendingRequestAfterActiveWorkFinishes() async {
        let scheduler = PreviewScheduler()
        let started = Gate()
        let release = Gate()
        var events: [String] = []

        scheduler.schedule {
            events.append("first started")
            started.open()
            await release.wait()
            events.append("first finished")
        }
        await started.wait()

        scheduler.schedule { events.append("superseded") }
        let latest = scheduler.schedule { events.append("latest finished") }
        release.open()
        await latest.value

        #expect(events == ["first started", "first finished", "latest finished"])
    }

    @Test(.timeLimit(.minutes(1)))
    func cancellingSkipsPendingWork() async {
        let scheduler = PreviewScheduler()
        let started = Gate()
        let release = Gate()
        var events: [String] = []

        scheduler.schedule {
            started.open()
            await release.wait()
            events.append("active finished")
        }
        await started.wait()

        let pending = scheduler.schedule { events.append("cancelled") }
        scheduler.cancel()
        release.open()
        await pending.value

        #expect(events == ["active finished"])
    }

    @Test(.timeLimit(.minutes(1)))
    func cancellingActiveWorkAllowsANewPreviewWithoutPublishingTheOldOne() async {
        let scheduler = PreviewScheduler()
        let started = Gate()
        let release = Gate()
        var previews: [String] = []

        scheduler.schedule {
            started.open()
            await release.wait()
            guard !Task.isCancelled else { return }
            previews.append("outdated")
        }
        await started.wait()
        scheduler.cancel()

        let latest = scheduler.schedule { previews.append("current") }
        release.open()
        await latest.value

        #expect(previews == ["current"])
    }
}
