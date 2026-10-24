yyterminal-editor / yyte — Initial developer preview

F1 / F10     Command menu (arrows, Enter; Esc closes)
F2           Editor
F3           Git status
F4           Integrated shell
F5           Markdown preview
F6           Switch explorer/editor focus
Ctrl+O / P   Open a file by path
Ctrl+N       New document
Ctrl+Tab     Next document
Ctrl+W       Close saved document
Ctrl+C/X/V   Internal clipboard (up to 1 MiB; not the system clipboard)
Ctrl+S       Save (asks for a path on a new document)
Ctrl+F       Find next literal text (next 16 MiB)
Ctrl+Z / Y   Undo / redo
Ctrl+A       Select all
Shift+arrows Extend selection
Ctrl+Home/End Document start / end
Ctrl+Q       Quit; modified tabs require typing discard

The menu also provides Save as, Git diff/staged diff, stage all,
unstage all, commit staged changes, Help, and Markdown preview.
Git operations affect the explorer's current directory. Review staged
changes before committing. Git, a shell, and a UTF-8 terminal are required.
Fonts are controlled by your terminal; no font is included.

Shell: input is sent to a real PTY. F1–F6, F10 and Ctrl+Q are reserved
for the application. F2 returns to editing. Bracketed paste is supported.
Shift+PageUp/PageDown scroll through 2,000 lines of shell history.
Typing returns to the live screen. The app-owned shell is stopped on exit.

Files: opened on a worker into a private disk snapshot. Saving checks the
original SHA-256 and refuses external changes. Save as refuses existing
paths. Private copies need free temporary storage comparable to file size.
Rendering reads bounded visible rows; long lines wrap. Auto-detected YY
encodings and original invalid bytes are preserved by the shared model.
Loading and saving run off the UI thread.

Encoding: yyte --encoding IBM-930/fixed:80 FILE
Use yyte --list-encodings to list shared YY encodings. The Open encoding
menu sets the encoding for the next Open file (auto restores detection).
Existing documents keep their own encoding; saving preserves it.

Open a .yyworkspace file to list its folders. Workspace folders returns
to this list. Relative paths use the workspace file directory. SSH and
foreign OS paths are preserved, but cannot be opened in this preview.
The workspace file is never rewritten.

Concurrent external writers are not locked: stop other writers before
saving important data.

Initial limitations: no system clipboard, mouse selection, remote SSH,
structured CSV/fixed-record editing, or YY binary file editing yet.
Workspace loading is supported; creating/saving workspaces is not yet.
Markdown preview is text-only and limited to the first 1 MiB; diagrams/HTML are not executed. Git output is
currently captured in memory. Giant repository diffs are not optimized.
