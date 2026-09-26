import AppKit
import Darwin
import Foundation

private let executableStartedAt = ProcessInfo.processInfo.systemUptime

private struct Entry {
    let id: Int
    let name: String
    let size: UInt64
    let modified: Int
    let isDirectory: Bool
}

private func elapsedMilliseconds(since start: TimeInterval) -> Double {
    (ProcessInfo.processInfo.systemUptime - start) * 1_000
}

private func generateEntries(count: Int) -> [Entry] {
    let extensions = ["txt", "json", "rs", "swift", "png", "pdf", "log"]
    return (0..<count).map { index in
        let isDirectory = index % 11 == 0
        let stem = String(format: isDirectory ? "folder-%06d" : "report-%06d", index)
        let name = isDirectory ? stem : "\(stem).\(extensions[index % extensions.count])"
        let size = (UInt64(index) * 2_654_435_761) % 50_000_000 + 1_024
        return Entry(
            id: index,
            name: name,
            size: size,
            modified: 1_700_000_000 + index % 31_536_000,
            isDirectory: isDirectory
        )
    }
}

private func residentMemoryBytes() -> UInt64 {
    var info = mach_task_basic_info()
    var count = mach_msg_type_number_t(
        MemoryLayout<mach_task_basic_info_data_t>.size / MemoryLayout<natural_t>.size
    )
    let result = withUnsafeMutablePointer(to: &info) { pointer in
        pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { rebound in
            task_info(
                mach_task_self_,
                task_flavor_t(MACH_TASK_BASIC_INFO),
                rebound,
                &count
            )
        }
    }
    return result == KERN_SUCCESS ? UInt64(info.resident_size) : 0
}

private final class FileListViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    let tableView = NSTableView()
    var entries: [Entry] = []

    override func loadView() {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true

        tableView.rowHeight = 28
        tableView.headerView = NSTableHeaderView()
        tableView.usesAlternatingRowBackgroundColors = true
        tableView.allowsMultipleSelection = true
        tableView.dataSource = self
        tableView.delegate = self

        let columns: [(String, String, CGFloat)] = [
            ("name", "Name", 620),
            ("size", "Size", 150),
            ("modified", "Modified", 180),
        ]
        for (identifier, title, width) in columns {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(identifier))
            column.title = title
            column.width = width
            tableView.addTableColumn(column)
        }

        scrollView.documentView = tableView
        view = scrollView
    }

    func numberOfRows(in tableView: NSTableView) -> Int {
        entries.count
    }

    func tableView(
        _ tableView: NSTableView,
        viewFor tableColumn: NSTableColumn?,
        row: Int
    ) -> NSView? {
        guard row < entries.count, let tableColumn else { return nil }
        let identifier = tableColumn.identifier
        let cell = tableView.makeView(withIdentifier: identifier, owner: self) as? NSTableCellView
            ?? makeCell(identifier: identifier)
        let entry = entries[row]

        switch identifier.rawValue {
        case "size":
            cell.textField?.stringValue = entry.isDirectory ? "--" : entry.size.formatted()
        case "modified":
            cell.textField?.stringValue = String(entry.modified)
        default:
            cell.textField?.stringValue = entry.name
        }
        return cell
    }

    private func makeCell(identifier: NSUserInterfaceItemIdentifier) -> NSTableCellView {
        let cell = NSTableCellView()
        cell.identifier = identifier
        let label = NSTextField(labelWithString: "")
        label.lineBreakMode = .byTruncatingMiddle
        label.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(label)
        cell.textField = label
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 6),
            label.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -6),
            label.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }
}

private final class BenchmarkDelegate: NSObject, NSApplicationDelegate {
    private let controller = FileListViewController()
    private var window: NSWindow?
    private let entryCount: Int

    override init() {
        entryCount = Int(ProcessInfo.processInfo.environment["BENCHMARK_ENTRIES"] ?? "") ?? 100_000
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1_050, height: 700),
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "Furman AppKit File List Benchmark"
        window.contentViewController = controller
        window.orderFrontRegardless()
        self.window = window

        let generationStart = ProcessInfo.processInfo.systemUptime
        let baseline = generateEntries(count: entryCount)
        let generationMs = elapsedMilliseconds(since: generationStart)

        let populationStart = ProcessInfo.processInfo.systemUptime
        controller.entries = baseline
        controller.tableView.reloadData()
        window.displayIfNeeded()

        DispatchQueue.main.async { [self] in
            let initialPopulationMs = elapsedMilliseconds(since: populationStart)
            let startupToFirstRenderMs = elapsedMilliseconds(since: executableStartedAt)
            runOperations(
                baseline: baseline,
                generationMs: generationMs,
                initialPopulationMs: initialPopulationMs,
                startupToFirstRenderMs: startupToFirstRenderMs
            )
        }
    }

    private func runOperations(
        baseline: [Entry],
        generationMs: Double,
        initialPopulationMs: Double,
        startupToFirstRenderMs: Double
    ) {
        guard let window else { return }

        let sortStart = ProcessInfo.processInfo.systemUptime
        controller.entries = baseline.sorted { $0.size > $1.size }
        controller.tableView.reloadData()
        window.displayIfNeeded()
        let sortMs = elapsedMilliseconds(since: sortStart)

        let filterStart = ProcessInfo.processInfo.systemUptime
        controller.entries = baseline.filter { $0.name.lowercased().contains("report-099") }
        controller.tableView.reloadData()
        window.displayIfNeeded()
        let filterMs = elapsedMilliseconds(since: filterStart)
        let filteredCount = controller.entries.count

        controller.entries = baseline
        controller.tableView.reloadData()
        window.displayIfNeeded()

        let selectionCount = min(10_000, baseline.count)
        let selectionStart = ProcessInfo.processInfo.systemUptime
        controller.tableView.selectRowIndexes(
            IndexSet(integersIn: 0..<selectionCount),
            byExtendingSelection: false
        )
        window.displayIfNeeded()
        let bulkSelectionMs = elapsedMilliseconds(since: selectionStart)

        let smallStepScrollStart = ProcessInfo.processInfo.systemUptime
        if baseline.count > 1 {
            controller.tableView.scrollRowToVisible(0)
            window.displayIfNeeded()
            for step in 1...200 {
                let row = min(step * 3, baseline.count - 1)
                controller.tableView.scrollRowToVisible(row)
                window.displayIfNeeded()
            }
        }
        let smallStepScrollMs = elapsedMilliseconds(since: smallStepScrollStart)

        let scrollStart = ProcessInfo.processInfo.systemUptime
        if baseline.count > 1 {
            for step in 0..<200 {
                let row = step * (baseline.count - 1) / 199
                controller.tableView.scrollRowToVisible(row)
                window.displayIfNeeded()
            }
        }
        let scrollMs = elapsedMilliseconds(since: scrollStart)

        let result: [String: Any] = [
            "implementation": "appkit-nstableview",
            "entries": entryCount,
            "generation_ms": generationMs,
            "startup_to_first_render_ms": startupToFirstRenderMs,
            "initial_population_ms": initialPopulationMs,
            "sort_size_desc_ms": sortMs,
            "filter_name_ms": filterMs,
            "filtered_entries": filteredCount,
            "bulk_select_10000_ms": bulkSelectionMs,
            "scroll_200_small_steps_ms": smallStepScrollMs,
            "scroll_200_jumps_ms": scrollMs,
            "resident_memory_bytes": residentMemoryBytes(),
        ]

        let data = try! JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
        print(String(decoding: data, as: UTF8.self))
        fflush(stdout)
        NSApplication.shared.terminate(nil)
    }
}

let application = NSApplication.shared
private let delegate = BenchmarkDelegate()
application.setActivationPolicy(.regular)
application.delegate = delegate
application.run()
