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

Shell: input is sent to a real PTY. F1–F7, F10 and Ctrl+Q are reserved
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

Initial limitations: no remote SSH editing yet. Mouse cell selection and
.yys workbook editing are available; unsupported YY binary formats remain blocked.
Workspace loading is supported; creating/saving workspaces is not yet.
Markdown preview is text-only and limited to the first 1 MiB; diagrams/HTML are not executed. Git output is
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
the byte-preserving grid uniformly for this session. Use Spreadsheet mode
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
