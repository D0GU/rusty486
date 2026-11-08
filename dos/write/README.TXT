R++ WRITE 2.0
=============

A word processor for writing books, for TinyDOS on rusty486. It is written
in R++ (a subset of Rust) and built with Rustland R++; the source is
C:\RPP\WRITE.RS.

START
  WRITE              start with the welcome document
  WRITE SAMPLE       open SAMPLE.WP, a short sample book
  WRITE BOOK.TXT     import a plain text or Markdown file

MENUS
  F10, or Alt + the highlighted letter: File, Edit, Search, Format, Style,
  View, Book, Options, Help. Arrows move, Enter chooses, Esc closes.

FILES (File menu)
  New, Open (with a list of .WP and .TXT files), Save, Save as, Export
  text, Rename file, Delete file, Exit. Documents are saved as .WP files,
  which keep all formatting. Export writes plain text with Markdown-style
  headings (# chapter, ## section, > quote), which Open reads back.
  R++ Write asks before discarding unsaved changes.

KEYS
  Ctrl+N new        Ctrl+O open       Ctrl+S save       Alt+X exit
  Shift+arrows select               Ctrl+X/C/V cut, copy, paste
  Ctrl+Z undo       Ctrl+A select all Ctrl+Y delete paragraph
  Ctrl+F find       F3 find next      Ctrl+H replace all
  Ctrl+G go to page Ctrl+Home/End     Ctrl+PgUp/PgDn previous/next chapter
  Ctrl+B bold       Ctrl+I italic     Ctrl+U underline  Ctrl+P plain
  Ctrl+L left       Ctrl+E centre     Ctrl+R right      Ctrl+J justify
  Alt+1 normal      Alt+2 chapter     Alt+3 section
  Alt+4 block quote Alt+5 scene break Alt+6 writer's note
  F1 keys           F5 outline        F7 focus mode     F9 statistics

  Style keys work on the selection, or switch the style of new text.

BOOK WRITING
  Chapter and section headings make up the outline (F5), which lists the
  words and page of each chapter and jumps to it. Writer's notes stay in the
  document but out of the word count and the export. Pages are marked as
  you write (View > Page breaks; Options > Page length). The status bar
  shows the page, line, word count and progress towards a session word
  goal (Book > Word goal). Focus mode (F7) hides everything but the text;
  typewriter scrolling keeps the current line in the middle of the screen.

OPTIONS
  Colors (Borland Blue, Paper, Amber, Midnight), first-line indent,
  auto-save (every 300 edits once a document has a name), page length.
  Options > Save options writes WRITE.CFG in the current directory.

LIMITS
  200,000 characters per document (about 33,000 words): keep long books
  as one file per part or a few chapters per file. Three undo steps.
