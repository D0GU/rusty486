TEXT 1.0
========

A Notepad-style editor for plain text files (README files, AUTOEXEC.BAT,
assembly and R++ sources, notes), for TinyDOS on rusty486. It is written
in R++ and built with Rustland R++; the source is C:\RPP\TEXT.RS.
For formatted documents and books, use R++ Write (C:\WRITE).

START
  TEXT               a new, untitled file
  TEXT NOTES.TXT     open NOTES.TXT (or start it, if it doesn't exist)

FILES
  Text keeps a file exactly as it was: CR LF or LF line ends (shown in the
  status bar), tab characters, and a missing newline at the end. Save As
  adds .TXT when you give no extension. Files up to 2 MB.

MENUS
  F10, or Alt + the highlighted letter: File, Edit, Search, View, Help.

KEYS
  Ctrl+N new        Ctrl+O open       Ctrl+S save       Alt+X exit
  Shift+arrows select               Ctrl+X/C/V cut, copy, paste
  Ctrl+Z undo       Ctrl+A select all Ins insert/overwrite
  Ctrl+F find       F3 find next      Ctrl+H replace all
  Ctrl+G go to line F5 time and date  Ctrl+W word wrap
  Ctrl+Left/Right word  Home/End line  Ctrl+Home/End file  PgUp/PgDn page

VIEW
  Word wrap, line numbers, status bar and tab width (8, 4 or 2). The
  settings are saved in TEXT.CFG in the current directory.
