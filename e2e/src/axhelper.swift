import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation

/// A request the helper could not serve. Its message is the reply's error.
struct Failure: Error {
    let message: String
}

let processMatch = CommandLine.arguments[1]
let windowTitle = CommandLine.arguments[2]
let overlayTitle = CommandLine.arguments[3]

let buttonRoles: Set<String> = [
    "AXButton", "AXCheckBox", "AXRadioButton", "AXPopUpButton", "AXMenuButton", "AXMenuItem",
]
let textRoles: Set<String> = ["AXStaticText", "AXButton", "AXCheckBox", "AXRadioButton"]
let lookupDepth = 10
let namedDepth = 12
let valueSettle: useconds_t = 300_000
let focusSettle: useconds_t = 500_000
let dragPause: useconds_t = 30_000
let dragSteps = 20
let notTrusted = "assistive access is not granted to the process running the e2e suite"

func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else {
        return nil
    }
    return value
}

func string(_ element: AXUIElement, _ key: String) -> String {
    attribute(element, key) as? String ?? ""
}

func children(_ element: AXUIElement) -> [AXUIElement] {
    attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? []
}

func role(_ element: AXUIElement) -> String {
    string(element, kAXRoleAttribute)
}

/// The name System Events reports for an element: its title, which WebKit
/// fills from an aria-label, else a static text's value, else its description.
func name(_ element: AXUIElement) -> String {
    for key in [kAXTitleAttribute, kAXValueAttribute, kAXDescriptionAttribute] {
        let value = string(element, key)
        if !value.isEmpty {
            return value
        }
    }
    return ""
}

func frame(_ element: AXUIElement) -> CGRect? {
    guard let position = attribute(element, kAXPositionAttribute),
        let size = attribute(element, kAXSizeAttribute)
    else {
        return nil
    }
    var origin = CGPoint.zero
    var extent = CGSize.zero
    guard AXValueGetValue(position as! AXValue, .cgPoint, &origin),
        AXValueGetValue(size as! AXValue, .cgSize, &extent)
    else {
        return nil
    }
    return CGRect(origin: origin, size: extent)
}

/// The pid of the app under test: the process whose executable name holds
/// the match, the same process System Events finds by name.
func appPid() throws -> pid_t {
    let count = proc_listallpids(nil, 0)
    var pids = [pid_t](repeating: 0, count: Int(count) * 2)
    let filled = proc_listallpids(&pids, Int32(pids.count * MemoryLayout<pid_t>.size))
    var path = [CChar](repeating: 0, count: Int(MAXPATHLEN) * 4)
    for pid in pids.prefix(Int(max(filled, 0))) where pid > 0 && pid != getpid() {
        guard proc_pidpath(pid, &path, UInt32(path.count)) > 0 else {
            continue
        }
        let executable = URL(fileURLWithPath: String(cString: path)).lastPathComponent
        if executable.contains(processMatch) {
            return pid
        }
    }
    throw Failure(message: "the app is not running")
}

func firstChild(_ element: AXUIElement, role wanted: String?) -> AXUIElement? {
    children(element).first { wanted == nil || role($0) == wanted }
}

/// The page System Events reaches as UI element 1 of scroll area 1 of group 1
/// of group 1 of the main window, found by its title.
func pageRoot() throws -> AXUIElement {
    let app = AXUIElementCreateApplication(try appPid())
    let windows = attribute(app, kAXWindowsAttribute) as? [AXUIElement] ?? []
    guard let window = windows.first(where: { string($0, kAXTitleAttribute) == windowTitle }) else {
        throw Failure(message: "the main window is not open")
    }
    guard let outer = firstChild(window, role: kAXGroupRole),
        let inner = firstChild(outer, role: kAXGroupRole),
        let scroll = firstChild(inner, role: kAXScrollAreaRole),
        let page = firstChild(scroll, role: nil)
    else {
        throw Failure(message: "the page is not loaded")
    }
    return page
}

func findButton(_ element: AXUIElement, _ wanted: String, _ depth: Int) -> AXUIElement? {
    if depth > lookupDepth {
        return nil
    }
    if buttonRoles.contains(role(element)) && name(element) == wanted {
        return element
    }
    for child in children(element) {
        if let found = findButton(child, wanted, depth + 1) {
            return found
        }
    }
    return nil
}

func hasText(_ element: AXUIElement, _ needle: String, _ depth: Int) -> Bool {
    if depth > lookupDepth {
        return false
    }
    if textRoles.contains(role(element)) && name(element).contains(needle) {
        return true
    }
    return children(element).contains { hasText($0, needle, depth + 1) }
}

/// Any element whose name starts with the needle or whose description is it:
/// a drag runs between rows and lanes that are not buttons.
func findNamed(_ element: AXUIElement, _ needle: String, _ depth: Int) -> AXUIElement? {
    if depth > namedDepth {
        return nil
    }
    if name(element).hasPrefix(needle) || string(element, kAXDescriptionAttribute) == needle {
        return element
    }
    for child in children(element) {
        if let found = findNamed(child, needle, depth + 1) {
            return found
        }
    }
    return nil
}

func button(_ wanted: String) throws -> AXUIElement {
    guard let found = findButton(try pageRoot(), wanted, 0) else {
        throw Failure(message: "button not found")
    }
    return found
}

func composer() throws -> AXUIElement {
    guard let area = firstChild(try pageRoot(), role: kAXTextAreaRole) else {
        throw Failure(message: "the composer is not on the page")
    }
    return area
}

func press(_ element: AXUIElement) throws {
    let result = AXUIElementPerformAction(element, kAXPressAction as CFString)
    guard result == .success else {
        throw Failure(message: "pressing failed (AXError \(result.rawValue))")
    }
}

func point(_ value: Any?) throws -> CGPoint {
    guard let pair = value as? [Double], pair.count == 2 else {
        throw Failure(message: "a point is two numbers")
    }
    return CGPoint(x: pair[0], y: pair[1])
}

/// Drags with real mouse events, in steps, so the page sees a press, pointer
/// moves past its drag threshold, and a release.
func drag(from start: CGPoint, to end: CGPoint) {
    func post(_ type: CGEventType, _ at: CGPoint) {
        CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: at, mouseButton: .left)?
            .post(tap: .cghidEventTap)
        usleep(dragPause)
    }
    post(.mouseMoved, start)
    post(.leftMouseDown, start)
    for step in 1...dragSteps {
        let t = Double(step) / Double(dragSteps)
        post(.leftMouseDragged, CGPoint(x: start.x + (end.x - start.x) * t, y: start.y + (end.y - start.y) * t))
    }
    post(.leftMouseUp, end)
}

func windows(_ option: CGWindowListOption) -> [[String: Any]] {
    CGWindowListCopyWindowInfo(option, kCGNullWindowID) as? [[String: Any]] ?? []
}

/// The id of the app's largest untitled or main window, which screencapture -l
/// grabs even when another Space is showing.
func mainWindowId() throws -> Int {
    var best: (id: Int, area: Int)?
    for window in windows(.optionAll) {
        let owner = (window[kCGWindowOwnerName as String] as? String ?? "").lowercased()
        guard owner.contains("inference-gateway") || owner.contains("inference gateway"),
            let bounds = window[kCGWindowBounds as String] as? [String: Int],
            let id = window[kCGWindowNumber as String] as? Int
        else {
            continue
        }
        let title = window[kCGWindowName as String] as? String ?? ""
        if !title.isEmpty && title != windowTitle {
            continue
        }
        let area = (bounds["Width"] ?? 0) * (bounds["Height"] ?? 0)
        if best == nil || area > best!.area {
            best = (id, area)
        }
    }
    guard let best else {
        throw Failure(message: "app window not found for screenshot")
    }
    return best.id
}

/// The overlay's bounds next to the ones it should have: the primary screen
/// below its menu bar, reaching the physical bottom edge behind the Dock.
func overlayBounds() -> [Double] {
    guard
        let overlay = windows(.optionOnScreenOnly).first(where: { window in
            let owner = window[kCGWindowOwnerName as String] as? String ?? ""
            let title = window[kCGWindowName as String] as? String ?? ""
            return owner.contains(processMatch) && title == overlayTitle
        }),
        let bounds = overlay[kCGWindowBounds as String] as? [String: CGFloat],
        let screen = NSScreen.screens.first
    else {
        return []
    }
    let full = screen.frame
    let expectedTop = full.maxY - screen.visibleFrame.maxY
    return [
        bounds["X"] ?? 0, bounds["Y"] ?? 0, bounds["Width"] ?? 0, bounds["Height"] ?? 0,
        full.minX, expectedTop, full.width, full.height - expectedTop,
    ].map { Double($0) }
}

func serve(_ request: [String: Any]) throws -> Any {
    let wanted = request["name"] as? String ?? ""
    switch request["op"] as? String ?? "" {
    case "ready":
        guard AXIsProcessTrusted() else {
            throw Failure(message: notTrusted)
        }
        _ = try pageRoot()
        return true
    case "exists_button":
        return findButton(try pageRoot(), wanted, 0) != nil
    case "exists_text":
        return hasText(try pageRoot(), wanted, 0)
    case "press":
        try press(try button(wanted))
        return true
    case "value":
        return string(try button(wanted), kAXValueAttribute)
    case "centre":
        guard let element = findNamed(try pageRoot(), wanted, 0), let box = frame(element) else {
            throw Failure(message: "element not found")
        }
        return [Double(box.midX), Double(box.midY)]
    case "above_composer":
        guard let buttonBox = frame(try button(wanted)), let areaBox = frame(try composer()) else {
            throw Failure(message: "no frame for the button or the composer")
        }
        return [Double(buttonBox.maxY), Double(areaBox.minY)]
    case "composer_get":
        return string(try composer(), kAXValueAttribute)
    case "composer_press":
        try press(try composer())
        return true
    case "composer_set":
        let area = try composer()
        if request["press"] as? Bool == true {
            try? press(area)
            usleep(focusSettle)
        }
        let text = request["text"] as? String ?? ""
        AXUIElementSetAttributeValue(area, kAXValueAttribute as CFString, text as CFString)
        usleep(valueSettle)
        return string(area, kAXValueAttribute)
    case "drag":
        drag(from: try point(request["from"]), to: try point(request["to"]))
        return true
    case "window_id":
        return try mainWindowId()
    case "overlay_bounds":
        return overlayBounds()
    case let op:
        throw Failure(message: "unknown op \(op)")
    }
}

func reply(to line: String) -> [String: Any] {
    var reply: [String: Any] = ["ok": false]
    do {
        guard let request = try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any] else {
            throw Failure(message: "a request is a JSON object")
        }
        reply["id"] = request["id"]
        reply["value"] = try serve(request)
        reply["ok"] = true
    } catch let failure as Failure {
        reply["error"] = failure.message
    } catch {
        reply["error"] = "\(error)"
    }
    return reply
}

while let line = readLine() {
    let answer = reply(to: line)
    let data =
        (try? JSONSerialization.data(withJSONObject: answer))
        ?? Data(#"{"ok":false,"error":"the reply could not be encoded"}"#.utf8)
    FileHandle.standardOutput.write(data + Data("\n".utf8))
}
