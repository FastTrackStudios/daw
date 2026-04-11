# iOS App Spec

## Lock Screen Widgets

### t[ios.widget.lockscreen]
The app provides native iOS lock screen widgets using WidgetKit. Widgets are updated via a shared App Group container so that data written by the main app is immediately available to the widget extension without launching the full app.

### t[ios.widget.lockscreen.display]
A display widget shows tasks on the lock screen. Three sizes must be supported:

**Rectangular (medium):**
- Shows the single highest-urgency open task: title, project name, and due date (if set)
- If no open tasks exist, shows a "Nothing due" empty state

**Rectangular (large):**
- Shows up to 3 open tasks sorted by urgency: title and due date per row
- Truncates title to one line; remaining tasks shown as a count ("+ 2 more") if the list exceeds 3

**Circular (small):**
- Shows the count of open tasks due today
- If zero, shows a checkmark

### t[ios.widget.lockscreen.display.filter]
The display widget supports a configurable filter set in widget settings:
- All open tasks (default)
- Tasks due today
- Tasks for a specific project (selected from a project picker)
- Tasks with a specific context

### t[ios.widget.lockscreen.display.tap]
Tapping the display widget deep-links into the app to the filtered task list that the widget is showing. If the device is locked, authentication is required first, then the deep link resolves.

### t[ios.widget.lockscreen.add]
A quick-add widget places a button on the lock screen that opens a task capture sheet without fully launching the app. The capture sheet supports:
- Task title (required, text input)
- Due date (optional, date picker)
- Project (optional, picker populated from recent/pinned projects)

Submitting the capture sheet creates the task and immediately dismisses. The task is written to the shared App Group container and synced to the vault on next full app launch or background sync.

### t[ios.widget.lockscreen.add.authentication]
The quick-add widget requires Face ID / Touch ID authentication before the capture sheet is presented, as it accesses vault data. Authentication is handled by the system lock screen; no additional prompt is shown if the device was just unlocked.

### t[ios.widget.lockscreen.refresh]
Widget timeline is refreshed:
- Immediately after the main app writes a task mutation to the shared container
- On a background refresh schedule (minimum interval 15 minutes, subject to iOS system throttling)
- After a task is completed via the widget itself

### t[ios.widget.homescreen]
The same widget configurations available on the lock screen are also available as Home Screen widgets (system permission permitting). Home screen widgets additionally support interactive completion checkboxes (iOS 17+ interactive widgets) so tasks can be checked off directly from the Home Screen without opening the app.

## Deep Links

### t[ios.deeplink]
The app registers a URL scheme and Universal Links for navigation from widgets and external sources. Supported deep link targets:
- `/tasks` — open task list
- `/tasks/new` — open new task capture sheet
- `/tasks/new?title=...&due=...&project=...` — pre-populated capture sheet
- `/projects` — open project dashboard
- `/projects/{id}` — open specific project detail
