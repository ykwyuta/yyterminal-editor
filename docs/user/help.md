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

Shell tabs: input is sent to a real PTY. F1–F7, F10 and Ctrl+Q are reserved
for the application. F2 returns to editing. Bracketed paste is supported.
Shift+PageUp/PageDown scroll through 2,000 lines of shell history.
Typing returns to the live screen. Terminal starts or selects a shell tab;
New terminal / +Terminal creates another. Background tabs keep their output.
Closing a terminal tab stops its app-owned shell; exiting stops all shells.
Ctrl+Tab and Ctrl+W also switch/close terminal tabs.

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

Initial limitations: no remote SSH editing yet. Mouse cell selection and
.yys workbook editing are available; unsupported YY binary formats remain blocked.
Workspace loading is supported; creating/saving workspaces is not yet.
Markdown preview is limited to the first 1 MiB; images become pixel art and
Mermaid becomes ASCII diagrams. HTML/JavaScript are not executed. Git output is
currently captured in memory. Giant repository diffs are not optimized.

Table editing (F7)
------------------
F7 toggles CSV/TSV table and text mode. The file extension chooses the
initial delimiter; the menu also offers CSV, TSV and Delimited table.
Delimited table accepts a 1–4 byte delimiter, including escapes such as
\t. Quotes and record newlines follow the shared YY CSV parser.

Arrows / Tab   Move between cells
Enter          Edit current cell; Ctrl+A clears the input
Alt+Enter      Insert a newline while editing a cell
Ctrl+C         Copy a cell in TSV notation to the internal clipboard
Ctrl+V / paste Paste TSV cells starting at the selected cell
F8 / F9        Delete current record / append a record at file end
PgUp / PgDn    Previous / next page
Ctrl+Z / Y     Undo / redo (a rectangular paste is one undo unit)
Ctrl+S         Save in the original encoding; queued during page loading

Pastes must fit within the current page and its existing fields. Every
value is checked before changing the document, so an invalid value does
not leave a partially pasted block. Enter editing supports literal cell
newlines; pasting directly on the grid interprets TSV records/columns.
Other fields, original quoting and record line endings remain intact.

Fixed-length editing
--------------------
Open a saved data file, then choose Fixed-length table in the menu.
Enter four semicolon-separated settings, for example:
    record.cpy;80;IBM-930;none
    record.cpy;13;MS932;crlf
The parts are copybook path; record length in BYTES; character set;
record separator (none/crlf/lf/nl). The copybook path is relative to the
current explorer directory. Save any text edits before switching.

A new tab opens the original bytes. The copybook supplies columns and
COBOL field types, including COMP-3 and binary numeric fields. Only the
edited field's bytes are changed; unrecognized fields/padding stay intact.
Invalid numeric bytes are shown as X'...' and can be edited in that form.
Overflow, fractional truncation, overlong/unencodable text are rejected.
F2/F7 keep fixed files in the safe byte-based table editor.

Fixed character sets: MS932 and the shared COBOL codec's IBM EBCDIC
CCSIDs. Numeric fields use big-endian binary / host format. Single
and multi-layout copybooks and .yyl/.yys are supported (see below).
The copybook may be shorter than the record length; remaining bytes are
preserved. Appended records initialize defined fields and fill gaps with
encoded spaces. Corrupt separators/truncated records produce an error.

Pages contain at most 128 records and 1 MiB of data, with up to 4096
fields. A CSV record crossing the page byte budget is left untouched;
use text mode if it is too large. Grid pages load on a worker. Column width and Row height commands adjust
individual columns/rows of the byte-preserving grid for this session. Use Spreadsheet mode
for individual columns/selected rows and persistent dimensions.
See demo/README.md for ready-to-use CSV and packed-decimal examples.


Spreadsheet workbooks (.yys)
---------------------------
Choose Spreadsheet mode to import the current CSV/TSV or fixed-length
snapshot into a NEW workbook tab using the yysheet engine. The source tab
remains available. New workbook creates an empty sheet. Open .yys reads an
existing workbook. Save / Save as writes .yys; Export CSV writes the active
sheet's visible values to a new UTF-8 CSV (original CSV quoting is not kept).

Enter edits a cell. Inputs beginning = are formulas. Shared yysheet formula
functions and references, recalculation, Undo/Redo, Autofill down, sort,
wildcard column filters, row/column insertion/deletion, Bold and Number
format are available from the command menu. Clear sort/filter before
structural edits. Move column currently requires a populated table without
formulas or style layers; unsafe reference changes are rejected.
Next sheet and Add sheet manage workbook sheets.

Shift+arrows or mouse drag selects a rectangle. Ctrl+C/X/V uses the app's
TSV clipboard; terminal bracketed paste accepts a rectangular TSV block.
Copy to system clipboard sends OSC 52 only when explicitly selected;
the terminal must allow it. Paste OS clipboard text using the terminal's
paste shortcut. Native OS clipboard reading is not implemented.

Column width accepts 4..80 terminal cells and applies to selected columns.
Row height accepts 1..20 terminal lines and applies to selected rows.
Multiline/long cell values wrap within those dimensions. Dimensions,
formulas and formatting survive .yys save/reopen. Height metadata is kept
in the companion sheet __yyte_view_v1; other YY apps may display this sheet.
The terminal UI hides it. Deleting it removes saved row heights.

Fixed layout files and inference
-------------------------------
Fixed-length table also accepts a .yyl path in place of the copybook:
    records.yyl;80;MS932;none
The record length must agree with the .yyl definition.
Apply layout (.yyl) loads a saved layout for the current on-disk data into
a new tab. Save layout (.yyl) writes a new shared YY layout file.
Infer fixed layout accepts LENGTH;CHARSET;SEPARATOR or auto;CHARSET;SEPARATOR.
It samples up to 1 MiB, proposes record lengths and multiple layouts, and
opens the editable .yyl source in a new text tab. Review/edit, Save as a
.yyl file, and Apply layout. Inference is a suggestion, not a guarantee.

Multi-layout imports open directly as workbooks. A layout column identifies
each record. Automatic assignment chooses the first name-sorted layout
that decodes without errors, not a discriminator VALUE match. Use Row
layout (or edit that cell) to choose another definition and re-decode the
record. Assign undetermined rows before export. Do not edit layout labels
without checking the resulting field types.

Fixed-length export preserving layout
------------------------------------
Choose Export fixed-length and enter a NEW output path. In the raw grid,
the exact current record bytes are copied, including packed/binary fields,
fillers, original encoding and separators. No text transcoding is applied.
The source file is never overwritten by export.

In a fixed workbook, fields map back to their original offsets and types,
even after moving columns. Each record carries read-only original bytes
in __YYTE_ORIGINAL_RECORD, persisted in .yys and hidden in this UI. Export
starts with these bytes and changes only fields whose values differ.
Untouched packed signs, invalid field bytes, gaps and trailing padding
therefore survive save/reopen/export. A shorter single copybook is extended
with a FILLER for the remaining record bytes when importing a workbook.
Other YY apps may display the reserved raw column; keep it intact.

Export follows current sort/filter order (filtered records are omitted).
Original final-separator absence is retained for the final record; a moved
record gains a separator if another record follows. New records initialize
fields and use encoded spaces. Overflow, conversion errors and fractional
loss at export are rejected; no partial destination is published. Formula
results in fixed fields follow the shared yysheet COBOL MOVE/type rules
(including that engine's numeric scaling). Unknown layouts require human
assignment before export. Workbooks without original bytes reconstruct
records from their layout and cannot recover historical filler bytes.

yymerge comparison
------------------
Compare file (yymerge) compares the current unsaved text snapshot with a
selected local file. It uses yymerge's shared large-file engine directly;
no separately installed yymerge executable is required. Open encoding
controls decoding of the compared file. Raw fixed tabs compare raw bytes.
Workbook comparisons export the active sheet's visible values to CSV;
.yys on the other side compares its first sheet. Formatting is not compared.

Left/right panels show line numbers and colored changes. N / P jump to the
next/previous changed row, arrows/PgUp/PgDn scroll, Home/End go to the ends.
F2 returns to the editor. The comparison has a disk-backed index and reads
only visible rows. Lines are displayed up to 4096 bytes; all bytes are
compared. Newline differences are shown as ␍ and ↵. Raw files without line
breaks appear as one diff row. Comparison is read-only; merging, intra-line
highlights and comparison cancellation are not available yet.


Markdown images, Mermaid and markdownViewer
------------------------------------------
F5 previews the current unsaved Markdown snapshot. Local images resolve
relative to the Markdown file's directory (untitled: explorer directory).
PNG/JPEG/WebP/GIF/BMP/ICO are decoded natively; animated GIF shows one frame.
Colored square pixels use two pixels per terminal cell via the half-block
character, so no Sixel/Kitty/browser graphics support is required. Truecolor
terminals give the best results. Transparent pixels blend on a dark base.
SVG, embedded HTML images and video are not supported; errors remain visible.
Images fit the current width (max 160 cells) and 32 terminal lines by default.
F5 rerenders changed image files; terminal resize recalculates image sizing.

Fenced mermaid blocks use a native Rust ASCII conversion engine.
Flowchart/graph, sequenceDiagram, erDiagram, classDiagram and basic
stateDiagram/stateDiagram-v2 are supported. Flowchart DAGs use a layered
layout in TB/TD/BT/LR/RL directions. Cycles and wide diagrams use a catalogue
of boxes and arrow cards with matching node IDs. Sequence diagrams use
participant lifelines and messages; ER/class display relationships and
attribute/member lists. Appearance and layout intentionally differ from Web
Mermaid. Shapes become boxes; colors/styles are omitted with a note. Subgraph
names are retained as notes. Unsupported syntax/diagram types show the
original source with a reason instead of inventing or hiding its meaning.
Limits: 64 nodes, 128 edges; dense sequence diagrams may fall back to source.

Standalone examples (install with cargo install --path . --locked):
    markdownViewer README.md
    markdownViewer --plain --width 100 demo/markdown-preview.md
    markdownViewer --print --color demo/markdown-preview.md
    cat README.md | markdownViewer --plain -
Interactive: arrows/PgUp/PgDn/Space/Home/End scroll; Left/Right pan;
q/Esc or Ctrl+C exits. Redirected output defaults to plain text with ASCII
image shading. --plain explicitly selects this mode. --color enables
truecolor half-blocks for printed output. --print avoids the interactive UI.
--base-dir DIR overrides relative image paths; --image-height N sets 1..120
terminal lines. Input is UTF-8, at most 1 MiB (BOM supported).

HTTP/HTTPS images require explicit opt-in: command Preview remote images in
yyte, or --remote-images in markdownViewer. Otherwise a placeholder explains
why the image is blocked. Downloads use TLS validation, a 5-second timeout,
at most 3 redirects and 20 MiB per resource. Local/data:base64 images have
the same 20 MiB cap; decoded images are limited to 8192x8192 and 64 MiB.
At most 16 image references and 20,000 output lines are rendered per preview.
Do not enable remote images if the document's URLs should not be contacted.

See docs/user/markdown-viewer.md and demo/markdown-preview.md for examples,
syntax coverage and the native renderer's differences from browser Mermaid.


Mouse menu and explorer navigation
----------------------------------
Click Menu in the top bar to open commands; click a command to run it.
Move over a menu item to highlight it, and use the wheel to reach commands
below the visible area. Clicking outside the menu closes it without
activating the item underneath. Esc still closes the menu.

Click a folder once in Explorer to enter it; click [..] to go to its parent.
Click a file once to open it in an editor tab. Wheel over Explorer moves
through the list without opening entries. On narrow screens, click Files
(or choose Explorer in the command menu) to show the file list.
The other top-bar labels also respond to clicks. Right-click within the
main content opens the command menu. Existing grid drag selection remains
available. The terminal must forward mouse events (application mouse mode);
terminal-native selection typically requires holding Shift.


Mouse-first editing, tabs and split preview
------------------------------------------
The top bar uses names rather than function-key labels: Menu, Edit, Git,
Terminal, +Terminal, Preview, Split, Files, Table. Function keys remain
optional shortcuts. The tab strip below it contains documents and terminal
tabs together. Click a tab to activate it, x to close it, and [<]/[>] to
reach tabs outside the visible strip. Unsaved document tabs require saving
before closing. Terminal tab closure terminates its shell, not just its view.

Click in the text editor to place the caret; drag to select text. Positions
respect Japanese/wide characters, combining graphemes and expanded tabs.
Copy/Cut/Paste/Undo/Redo/Select all are also available in Menu. Copy/Paste
uses the internal clipboard; Copy to system clipboard remains a separate
OSC 52 operation. In a terminal tab, Copy copies the visible screen and
Paste sends the internal clipboard; document Undo/Cut are not terminal actions.
Keyboard Ctrl+C still sends interrupt to the focused shell.

Input dialogs have [OK] and [Cancel] buttons. The unsaved-exit dialog has
[Discard] for an explicit mouse confirmation. Text/numbers still need input;
terminal-native paste can supply text. Buttons preserve the same validation
as keyboard confirmation. Tab/menu clicks cannot bypass an active dialog.

Wheel over text scrolls without moving the caret; over Preview/Help/Git/diff
it scrolls those views, and over Terminal it moves through shell history.
The grid supports dragging | in the column header to change column width,
and _ at the bottom of a row-number gutter to change row height. Dimensions
preview during dragging; release commits a single workbook undo operation.
Workbook dimensions persist in .yys, while raw CSV/fixed-grid dimensions are
session-only. Menu width/height commands remain available for exact values.

Click Split (or Split Markdown preview in Menu) while a Markdown document
is active to show its editor and preview together. The preview follows the
unsaved document and has independent wheel scrolling. Click either pane to
focus it. Split again or the preview's [x] closes the side pane; Preview/F5
still offers the full preview. Narrow screens stack the panes vertically;
very small screens show the editor until there is space for both.

Standalone markdownViewer also captures mouse input. Wheel scrolls, and
[Close], [Up], [Down], [Top], [End], [<], [>] provide clickable navigation.
Terminal-native text selection generally uses Shift while mouse capture is
active. Terminal emulators must forward mouse events to support these actions.
